//! Public input admission, request-lane transport construction and capture consumption.
use super::*;

impl NetworkSession {
    pub(super) fn prepare_request_lane(&mut self, lane: usize) -> Result<(), RequestError> {
        if self.request_queue_policy == RequestQueuePolicy::SharedFifo {
            return Ok(());
        }
        if let Some(existing) = self.request_lanes.get(&lane) {
            if existing
                .queues
                .iter()
                .any(|queue| !self.transport.queues()[*queue].is_empty())
            {
                return Err(RequestError::Busy);
            }
            return Ok(());
        }

        let mut request_lane = RequestLane {
            inputs: BTreeMap::new(),
            input_ports: BTreeMap::new(),
            outputs: BTreeMap::new(),
            captures: BTreeMap::new(),
            queues: Vec::new(),
        };
        let canonical_count = self
            .queue_lanes
            .iter()
            .take_while(|lane| lane.is_none())
            .count();
        for canonical in 0..canonical_count {
            let queue = self
                .transport
                .add_queue(self.transport.queues()[canonical].empty_like());
            let endpoints = self.endpoints[canonical].clone();
            match &endpoints.0 {
                QueueEndpoint::Process { index, port } => {
                    request_lane
                        .outputs
                        .entry((*index, port.clone()))
                        .or_default()
                        .push(queue);
                }
                QueueEndpoint::Input(name) => {
                    request_lane
                        .inputs
                        .entry(name.clone())
                        .or_default()
                        .push(queue);
                }
                QueueEndpoint::Capture(_) => unreachable!("capture cannot be a writer"),
            }
            match &endpoints.1 {
                QueueEndpoint::Process { index, port } => {
                    let position = self.processes[*index]
                        .inputs
                        .iter()
                        .position(|input| &input.name == port)
                        .unwrap();
                    self.processes[*index].inputs[position].queues.push(queue);
                    request_lane
                        .input_ports
                        .entry((*index, position))
                        .or_default()
                        .push(queue);
                }
                QueueEndpoint::Capture(name) => {
                    request_lane.captures.insert(name.clone(), queue);
                }
                QueueEndpoint::Input(_) => unreachable!("ingress cannot be a reader"),
            }
            self.endpoints.push(endpoints);
            self.queue_lanes.push(Some(lane));
            request_lane.queues.push(queue);
        }
        self.request_lanes.insert(lane, request_lane);
        Ok(())
    }

    /// Convert a typed public input name and payload, then use ordinary admission.
    /// Actual exported name, type/schema, closure and credit checks still apply.
    /// Conversion precedes those checks; errors return the constructed Value.
    /// ```
    /// use phenix_graph::{InputKey, runtime::NetworkSession};
    /// fn admit(session: &mut NetworkSession) {
    ///     let _ = session.inject_key(InputKey::<i64>::new("input"), 7);
    /// }
    /// ```
    /// ```compile_fail,E0308
    /// use phenix_graph::{InputKey, runtime::NetworkSession};
    /// fn wrong(session: &mut NetworkSession) {
    ///     session.inject_key(InputKey::<i64>::new("input"), String::new());
    /// }
    /// ```
    /// ```compile_fail,E0308
    /// use phenix_graph::{OutputKey, runtime::NetworkSession};
    /// fn wrong(session: &mut NetworkSession) {
    ///     session.inject_key(OutputKey::<i64>::new("input"), 7);
    /// }
    /// ```
    pub fn inject_key<T: Send + Sync + 'static>(
        &mut self,
        key: crate::InputKey<T>,
        payload: T,
    ) -> Result<(), InjectionError> {
        self.inject(key.name(), key.value(payload))
    }

    pub fn inject_local<T: Send + Sync + 'static>(
        &mut self,
        name: &str,
        value: T,
    ) -> Result<(), InjectionError> {
        self.inject(name, Value::local(value))
    }
    pub fn inject(&mut self, name: &str, value: Value) -> Result<(), InjectionError> {
        self.supervise_configured_deferred();
        self.inject_scoped(name, value, None)
    }
    pub(super) fn inject_scoped(
        &mut self,
        name: &str,
        value: Value,
        request: Option<RequestId>,
    ) -> Result<(), InjectionError> {
        let attempt = (|| {
            if self.terminal.is_some() {
                return Err(ConnectionError::Cancelled);
            }
            let indices = match request {
                Some(request) if self.request_queue_policy == RequestQueuePolicy::BoundedLanes => {
                    self.requests
                        .get(&request)
                        .and_then(|scope| self.request_lanes.get(&scope.lane))
                        .and_then(|lane| lane.inputs.get(name))
                }
                _ => self.ingress.get(name),
            }
            .ok_or(ConnectionError::InvalidDestination)?;
            if value.identity() != self.compiled.interface().inputs()[name].value {
                return Err(ConnectionError::WrongType);
            }
            let targets: Vec<_> = indices.iter().map(|i| FanoutTarget::identity(*i)).collect();
            let mut packet = Packet::new(value.clone());
            if let Some(lease) = &self.lease {
                packet = packet.with_lease(lease.clone());
            }
            packet.metadata = Some(Arc::new(CaptureMetadata {
                session: self.session_id,
                generation: self.generation,
                process: None,
                output: name.into(),
                activation: 0,
                request,
                capture: None,
            }));
            let mut transaction =
                self.transport
                    .prepare(&packet, &targets)
                    .map_err(|e| match e {
                        crate::connection::PrepareError::Connection(e) => e,
                        _ => ConnectionError::WrongType,
                    })?;
            self.transport.commit(&mut transaction)
        })();
        match attempt {
            Ok(()) => {
                if let Some(request) = request {
                    self.note_request_ingress(request, name);
                }
                // Accepted packet fanout is an input event for every physical
                // target, including aliases of the exported ingress.
                let touched = self.ingress[name].clone();
                for queue in touched {
                    self.dirty_queue(queue);
                }
                self.refresh();
                Ok(())
            }
            Err(reason) => Err(InjectionError { reason, value }),
        }
    }
    pub fn close_input(&mut self, name: &str) -> Result<(), ConnectionError> {
        self.supervise_configured_deferred();
        let queues = self
            .ingress
            .get(name)
            .ok_or(ConnectionError::InvalidDestination)?
            .clone();
        for queue in queues {
            self.transport.close_writer(queue)?;
            self.dirty_queue(queue);
        }
        let lane_queues: Vec<_> = self
            .request_lanes
            .values()
            .flat_map(|lane| lane.inputs[name].iter().copied())
            .collect();
        for queue in lane_queues {
            self.transport.close_writer(queue)?;
            self.dirty_queue(queue);
        }
        self.refresh();
        Ok(())
    }
    pub fn take_output(&mut self, name: &str) -> Result<Option<Packet>, ConnectionError> {
        self.supervise_configured_deferred();
        let index = *self
            .captures
            .get(name)
            .ok_or(ConnectionError::InvalidDestination)?;
        self.take_capture(name, index)
    }

    pub(super) fn take_capture(
        &mut self,
        name: &str,
        index: usize,
    ) -> Result<Option<Packet>, ConnectionError> {
        let packet = self.transport.pop(index);
        if let Some(packet) = &packet {
            self.observe_capture(name, packet);
            self.dirty_queue(index);
        }
        self.refresh();
        Ok(packet)
    }
}
