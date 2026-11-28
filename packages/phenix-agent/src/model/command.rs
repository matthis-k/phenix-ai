//! A host-configured text model executable: UTF-8 prompt on stdin and UTF-8
//! response on stdout. No shell, tool protocol, or credential discovery.
//! I/O is synchronous. Use only trusted commands that terminate; there is
//! no deadline or process-tree supervision in this initial transport.

use super::{ModelBackend, ModelOutput, ModelProfile, ModelStream};
use crate::ModelInput;
use phenix_graph::Failure;
use std::{
    ffi::OsString,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdout, Command, Stdio},
    thread::{self, JoinHandle},
};

#[derive(Clone)]
pub struct CommandBackend {
    executable: PathBuf,
    args: Vec<OsString>,
    max_prompt_bytes: usize,
    max_response_bytes: usize,
}

impl CommandBackend {
    pub fn new(
        executable: impl Into<PathBuf>,
        args: Vec<OsString>,
        max_prompt_bytes: usize,
        max_response_bytes: usize,
    ) -> Self {
        Self {
            executable: executable.into(),
            args,
            max_prompt_bytes,
            max_response_bytes,
        }
    }
}

impl ModelBackend for CommandBackend {
    fn respond(&mut self, input: ModelInput, _: &ModelProfile) -> Result<ModelStream, Failure> {
        let ModelInput::Prompt(prompt) = input else {
            return Err(failure("command text model does not support tool feedback"));
        };
        if prompt.len() > self.max_prompt_bytes {
            return Err(failure("model prompt exceeds byte limit"));
        }
        let limit = u64::try_from(self.max_response_bytes)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| failure("model response limit too large"))?;
        let mut child = Command::new(&self.executable)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| failure(error.to_string()))?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        // Write and read concurrently so the two OS pipe capacities do not
        // impose an artificial request/response ordering.
        let writer = match thread::Builder::new().spawn(move || stdin.write_all(prompt.as_bytes()))
        {
            Ok(writer) => writer,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failure(error.to_string()));
            }
        };
        Ok(Box::new(CommandStream {
            child: Some(child),
            writer: Some(writer),
            stdout: BufReader::new(stdout.take(limit)),
            remaining: self.max_response_bytes,
            finished: false,
        }))
    }
}

struct CommandStream {
    child: Option<Child>,
    writer: Option<JoinHandle<std::io::Result<()>>>,
    stdout: BufReader<std::io::Take<ChildStdout>>,
    remaining: usize,
    finished: bool,
}

impl CommandStream {
    fn finish(&mut self) -> Result<ModelOutput, Failure> {
        let status = self
            .child
            .as_mut()
            .expect("live child")
            .wait()
            .map_err(|error| failure(error.to_string()))?;
        self.child.take();
        self.writer
            .take()
            .expect("live writer")
            .join()
            .map_err(|_| failure("model input writer panicked"))?
            .map_err(|error| failure(error.to_string()))?;
        if !status.success() {
            return Err(failure(format!("model command exited with {status}")));
        }
        Ok(ModelOutput::Complete)
    }
}

impl Iterator for CommandStream {
    type Item = Result<ModelOutput, Failure>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        let mut bytes = Vec::new();
        let result = match self.stdout.read_until(b'\n', &mut bytes) {
            Ok(0) => {
                self.finished = true;
                self.finish()
            }
            Ok(n) if n <= self.remaining => {
                self.remaining -= n;
                String::from_utf8(bytes)
                    .map(ModelOutput::Delta)
                    .map_err(|_| failure("model command produced invalid UTF-8"))
            }
            Ok(_) => Err(failure("model response exceeds byte limit")),
            Err(error) => Err(failure(error.to_string())),
        };
        if result.is_err() {
            self.finished = true;
        }
        Some(result)
    }
}

impl Drop for CommandStream {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // The child is reaped before joining its stdin writer. Commands
        // spawning descendants that inherit pipes are outside this contract.
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

fn failure(message: impl Into<String>) -> Failure {
    Failure::application_message(message)
}
