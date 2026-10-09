use super::ZmqOutboundResponse;
use std::collections::VecDeque;
use zeromq::PushSocket;

// At most 32 response sockets across idle and active deliveries, preserving
// room in the vendored transport's 64-connection process cap for command peers.
// Ownership moves into delivery tasks; no shared lock spans network I/O.
const RESPONSE_CONNECTION_LIMIT: usize = 32;

#[derive(PartialEq, Eq)]
pub(super) struct Route {
    endpoint: String,
    session: String,
    generation: String,
}

impl Route {
    pub(super) fn from_response(response: &ZmqOutboundResponse) -> Option<Self> {
        response.connection_id.as_ref().map(|generation| Self {
            endpoint: response.endpoint.clone(),
            session: response.envelope.session_id.clone(),
            generation: generation.clone(),
        })
    }
}

#[derive(Default)]
pub(super) struct ResponseConnections(VecDeque<(Route, PushSocket)>);

impl ResponseConnections {
    pub(super) fn take(&mut self, route: &Route) -> Option<PushSocket> {
        // A replaced SDK socket must never receive replies on the retired
        // connection, even when its endpoint and identity session are unchanged.
        self.0.retain(|(key, _)| {
            key.endpoint != route.endpoint
                || key.session != route.session
                || key.generation == route.generation
        });
        let index = self.0.iter().position(|(key, _)| key == route)?;
        self.0.remove(index).map(|(_, socket)| socket)
    }

    pub(super) fn insert(&mut self, route: Route, socket: PushSocket) {
        self.0.retain(|(key, _)| key != &route);
        if self.0.len() == RESPONSE_CONNECTION_LIMIT {
            self.0.pop_front();
        }
        self.0.push_back((route, socket));
    }

    pub(super) fn reserve_active(&mut self, active: usize) {
        while self.0.len() + active > RESPONSE_CONNECTION_LIMIT && !self.0.is_empty() {
            self.0.pop_front();
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeromq::Socket;

    #[test]
    fn idle_connection_history_is_bounded_and_generation_replacement_discards_old_socket() {
        let mut connections = ResponseConnections::default();
        for index in 0..1000 {
            connections.insert(
                Route { endpoint: index.to_string(), session: "s".into(), generation: "g".into() },
                PushSocket::new(),
            );
        }
        assert_eq!(connections.len(), RESPONSE_CONNECTION_LIMIT);
        let previous =
            Route { endpoint: "999".into(), session: "s".into(), generation: "g".into() };
        let next = Route { endpoint: "999".into(), session: "s".into(), generation: "new".into() };
        assert!(connections.take(&next).is_none());
        assert!(connections.take(&previous).is_none());
        assert_eq!(connections.len(), RESPONSE_CONNECTION_LIMIT - 1);
        connections.reserve_active(24);
        assert_eq!(connections.len(), 8);
    }
}
