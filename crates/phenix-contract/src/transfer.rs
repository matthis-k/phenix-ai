//! Incremental M1 transfer envelope for arbitrarily large logical byte values.
//!
//! A frame fits in one existing ABI borrowed buffer. A transfer spans as many
//! frames as needed. A transfer ID is NOT authority: the kernel must bind it
//! to the current caller, generation and ticket before admitting a frame.
use crate::{Code, Failure, MAX_BYTES};
use std::io::{self, Read};

const MAGIC: [u8; 4] = *b"PS\x01\0";
const HEADER: usize = 33;
pub const MAX_CHUNK: usize = MAX_BYTES - HEADER;
pub const DEFAULT_CHUNK: usize = 64 * 1024;

fn bad(message: &'static str) -> Failure {
    Failure::new(Code::SchemaRejected, message)
}

/// Opaque kernel-issued identity. An all-zero identity is always invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferId([u8; 16]);
impl TransferId {
    pub fn from_kernel(bytes: [u8; 16]) -> Result<Self, Failure> {
        if bytes == [0; 16] {
            return Err(bad("empty transfer identity"));
        }
        Ok(Self(bytes))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payload<'a> {
    Data(&'a [u8]),
    End,
}

/// The offset uses 64 bits; total logical size is not encoded in any frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    pub id: TransferId,
    pub offset: u64,
    pub payload: Payload<'a>,
}
impl Frame<'_> {
    pub fn encode(self) -> Result<Vec<u8>, Failure> {
        let (tag, bytes) = match self.payload {
            Payload::Data(bytes) if !bytes.is_empty() => (0u8, bytes),
            Payload::Data(_) => return Err(bad("zero-length data frame")),
            Payload::End => (1u8, &[][..]),
        };
        if bytes.len() > MAX_CHUNK {
            return Err(bad("stream frame exceeds inline ABI capacity"));
        }
        let len = u32::try_from(bytes.len()).map_err(|_| bad("stream length overflow"))?;
        let mut output = Vec::with_capacity(HEADER + bytes.len());
        output.extend_from_slice(&MAGIC);
        output.extend_from_slice(&self.id.0);
        output.extend_from_slice(&self.offset.to_be_bytes());
        output.push(tag);
        output.extend_from_slice(&len.to_be_bytes());
        output.extend_from_slice(bytes);
        Ok(output)
    }
}
impl<'a> Frame<'a> {
    pub fn decode(raw: &'a [u8]) -> Result<Self, Failure> {
        if raw.len() < HEADER || raw.len() > MAX_BYTES || raw[..4] != MAGIC {
            return Err(bad("invalid stream frame header or size"));
        }
        let id = TransferId::from_kernel(raw[4..20].try_into().map_err(|_| bad("stream id"))?)?;
        let offset = u64::from_be_bytes(raw[20..28].try_into().map_err(|_| bad("offset"))?);
        let len = u32::from_be_bytes(raw[29..33].try_into().map_err(|_| bad("length"))?) as usize;
        if raw.len() != HEADER + len {
            return Err(bad("invalid stream frame length"));
        }
        let payload = match (raw[28], len) {
            (0, 1..) => Payload::Data(&raw[HEADER..]),
            (1, 0) => Payload::End,
            _ => return Err(bad("invalid stream frame tag")),
        };
        Ok(Self {
            id,
            offset,
            payload,
        })
    }
}

/// Per-transfer sequencing, EOF, and *opt-in* logical byte limit.
/// Kernel caller/generation authorization is separate from this protocol check.
#[derive(Debug, Clone)]
pub struct Receiver {
    id: TransferId,
    offset: u64,
    ended: bool,
    user_max: Option<u64>,
}
impl Receiver {
    pub fn new(id: TransferId, user_max: Option<u64>) -> Self {
        Self {
            id,
            offset: 0,
            ended: false,
            user_max,
        }
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn ended(&self) -> bool {
        self.ended
    }
    pub fn admit<'a>(&mut self, frame: Frame<'a>) -> Result<Option<&'a [u8]>, Failure> {
        if self.ended || frame.id != self.id || frame.offset != self.offset {
            return Err(bad("foreign, replayed or out-of-order stream frame"));
        }
        match frame.payload {
            Payload::End => {
                self.ended = true;
                Ok(None)
            }
            Payload::Data(bytes) if !bytes.is_empty() && bytes.len() <= MAX_CHUNK => {
                let count = u64::try_from(bytes.len()).map_err(|_| bad("offset overflow"))?;
                let next = self
                    .offset
                    .checked_add(count)
                    .ok_or_else(|| bad("offset overflow"))?;
                if self.user_max.is_some_and(|max| next > max) {
                    return Err(bad("user-configured stream byte limit exceeded"));
                }
                self.offset = next;
                Ok(Some(bytes))
            }
            _ => Err(bad("invalid stream chunk")),
        }
    }
}

/// Read at most a chunk from an arbitrary Read source. The complete logical
/// content is never buffered; only the current small frame is allocated.
pub struct Producer<R> {
    source: R,
    id: TransferId,
    offset: u64,
    ended: bool,
}
impl<R: Read> Producer<R> {
    pub fn new(source: R, id: TransferId) -> Self {
        Self {
            source,
            id,
            offset: 0,
            ended: false,
        }
    }
    pub fn next_frame(&mut self, bytes_per_frame: usize) -> io::Result<Option<Vec<u8>>> {
        if self.ended {
            return Ok(None);
        }
        let mut data = vec![0; bytes_per_frame.clamp(1, MAX_CHUNK)];
        let count = self.source.read(&mut data)?;
        data.truncate(count);
        let frame = if count == 0 {
            self.ended = true;
            Frame {
                id: self.id,
                offset: self.offset,
                payload: Payload::End,
            }
        } else {
            let previous = self.offset;
            self.offset = self.offset.checked_add(count as u64).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "stream offset overflow")
            })?;
            Frame {
                id: self.id,
                offset: previous,
                payload: Payload::Data(&data),
            }
        };
        frame
            .encode()
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(n: u8) -> TransferId {
        TransferId::from_kernel([n; 16]).unwrap()
    }

    #[test]
    fn multi_megabyte_value_is_streamed_in_bounded_frames() {
        let input = vec![0x5au8; 2 * 1024 * 1024 + 7];
        let mut producer = Producer::new(input.as_slice(), id(1));
        let mut receiver = Receiver::new(id(1), None);
        let mut result = 0;
        while let Some(bytes) = producer.next_frame(DEFAULT_CHUNK).unwrap() {
            assert!(bytes.len() <= DEFAULT_CHUNK + HEADER);
            let frame = Frame::decode(&bytes).unwrap();
            if let Some(chunk) = receiver.admit(frame).unwrap() {
                assert!(chunk.iter().all(|b| *b == 0x5a));
                result += chunk.len();
            }
        }
        assert_eq!(result, input.len());
        assert!(receiver.ended());
    }

    #[test]
    fn rejects_cross_stream_replay_and_unexpected_eof() {
        let mut receiver = Receiver::new(id(1), None);
        let frame = Frame {
            id: id(1),
            offset: 0,
            payload: Payload::Data(b"ab"),
        };
        assert_eq!(receiver.admit(frame).unwrap(), Some(b"ab".as_slice()));
        assert!(receiver.admit(frame).is_err());
        assert!(receiver
            .admit(Frame {
                id: id(2),
                offset: 2,
                payload: Payload::End
            })
            .is_err());
        assert!(receiver
            .admit(Frame {
                id: id(1),
                offset: 3,
                payload: Payload::End
            })
            .is_err());
        assert_eq!(
            receiver
                .admit(Frame {
                    id: id(1),
                    offset: 2,
                    payload: Payload::End
                })
                .unwrap(),
            None
        );
        assert!(receiver
            .admit(Frame {
                id: id(1),
                offset: 2,
                payload: Payload::End
            })
            .is_err());
    }

    #[test]
    fn malformed_frames_and_explicit_limits_fail_closed() {
        let frame = Frame {
            id: id(1),
            offset: u32::MAX as u64 + 1,
            payload: Payload::Data(b"hello"),
        };
        let bytes = frame.encode().unwrap();
        assert_eq!(Frame::decode(&bytes).unwrap(), frame);
        let mut corrupt = bytes.clone();
        corrupt[28] = 2;
        assert!(Frame::decode(&corrupt).is_err());
        let mut corrupt = bytes.clone();
        corrupt[32] = 2;
        assert!(Frame::decode(&corrupt).is_err());
        let mut limited = Receiver::new(id(1), Some(4));
        assert!(limited
            .admit(Frame {
                id: id(1),
                offset: 0,
                payload: Payload::Data(b"hello")
            })
            .is_err());
        assert_eq!(limited.offset(), 0);
        assert!(TransferId::from_kernel([0; 16]).is_err());
    }
}
