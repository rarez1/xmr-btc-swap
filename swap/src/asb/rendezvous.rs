use crate::rendezvous::XmrBtcNamespace;
use libp2p::core::connection::ConnectionId;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::rendezvous::{Event, Namespace};
use libp2p::swarm::{
    IntoProtocolsHandler, NetworkBehaviour, NetworkBehaviourAction, NetworkBehaviourEventProcess,
    PollParameters, ProtocolsHandler,
};
use libp2p::{Multiaddr, PeerId};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

#[derive(Debug)]
enum ConnectionState {
    Dialed,
    Connected,
    Disconnected,
}

/// A `NetworkBehaviour` that handles registration of the xmr-btc swap service
/// with a rendezvous point
pub struct Behaviour {
    rendezvous_behaviour: libp2p::rendezvous::Rendezvous,
    rendezvous_point_peer_id: PeerId,
    rendezvous_point_addr: Multiaddr,
    rendezvous_namespace: XmrBtcNamespace,
    rendezvous_reregister_timestamp: Option<Instant>,
    rendezvous_node_connection: ConnectionState,
    is_initial_registration: bool,
    events: Vec<NetworkBehaviourAction<BehaviourInEvent, ()>>,
}

impl Behaviour {
    pub fn new(
        keypair: Keypair,
        peer_id: PeerId,
        addr: Multiaddr,
        namespace: XmrBtcNamespace,
    ) -> Self {
        Self {
            rendezvous_behaviour: libp2p::rendezvous::Rendezvous::new(
                keypair,
                libp2p::rendezvous::Config::default(),
            ),
            rendezvous_point_peer_id: peer_id,
            rendezvous_point_addr: addr,
            rendezvous_namespace: namespace,
            rendezvous_reregister_timestamp: None,
            rendezvous_node_connection: ConnectionState::Disconnected,
            is_initial_registration: true,
            events: vec![],
        }
    }

    fn register(&mut self) {
        self.rendezvous_behaviour.register(
            Namespace::new(self.rendezvous_namespace.to_string())
                .expect("our namespace to be a correct string"),
            self.rendezvous_point_peer_id,
            None,
        );
    }

    pub fn refresh(&mut self) {
        match self.rendezvous_node_connection {
            ConnectionState::Dialed => {} /* we are waiting for a connection to be established,
                                            * no refresh */
            ConnectionState::Connected => {
                if let Some(rendezvous_reregister_timestamp) = self.rendezvous_reregister_timestamp
                {
                    if Instant::now() > rendezvous_reregister_timestamp
                        && !self.is_initial_registration
                    {
                        tracing::debug!("Sending re-registration to rendezvous node");
                        self.register();
                    }
                } else if self.is_initial_registration {
                    tracing::debug!("Sending initial registration to rendezvous node");
                    self.is_initial_registration = false;
                    self.register();
                }
            }
            ConnectionState::Disconnected => {
                let p2p_suffix = Protocol::P2p(self.rendezvous_point_peer_id.into());
                let address_with_p2p = if !self
                    .rendezvous_point_addr
                    .ends_with(&Multiaddr::empty().with(p2p_suffix.clone()))
                {
                    self.rendezvous_point_addr.clone().with(p2p_suffix)
                } else {
                    self.rendezvous_point_addr.clone()
                };

                self.events.push(NetworkBehaviourAction::DialAddress {
                    address: address_with_p2p,
                });

                self.rendezvous_node_connection = ConnectionState::Dialed;
            }
        }
    }
}

type BehaviourInEvent =
<<<libp2p::rendezvous::Rendezvous as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InEvent;

impl NetworkBehaviour for Behaviour {
    type ProtocolsHandler = <libp2p::rendezvous::Rendezvous as NetworkBehaviour>::ProtocolsHandler;
    type OutEvent = ();

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        <libp2p::rendezvous::Rendezvous as NetworkBehaviour>::ProtocolsHandler::default()
    }

    fn addresses_of_peer(&mut self, _peer_id: &PeerId) -> Vec<Multiaddr> {
        vec![]
    }

    fn inject_connected(&mut self, peer_id: &PeerId) {
        if *peer_id == self.rendezvous_point_peer_id {
            self.rendezvous_node_connection = ConnectionState::Connected;
        }
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        if *peer_id == self.rendezvous_point_peer_id {
            self.rendezvous_node_connection = ConnectionState::Disconnected;
            self.is_initial_registration = false;
        }
    }

    fn inject_event(
        &mut self,
        _peer_id: PeerId,
        _connection: ConnectionId,
        _event: <<libp2p::rendezvous::Rendezvous as NetworkBehaviour>::ProtocolsHandler as ProtocolsHandler>::OutEvent,
    ) {
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        _params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<BehaviourInEvent, Self::OutEvent>> {
        if let Some(event) = self.events.pop() {
            return Poll::Ready(event);
        }
        Poll::Pending
    }
}

impl NetworkBehaviourEventProcess<libp2p::rendezvous::Event> for Behaviour {
    fn inject_event(&mut self, event: Event) {
        match event {
            Event::RegisterFailed(error) => {
                self.is_initial_registration = false;
                tracing::error!(rendezvous_node=%self.rendezvous_point_peer_id, "Registration with rendezvous node failed: {:#}", error);
            }
            Event::Registered {
                rendezvous_node,
                ttl,
                namespace,
            } => {
                self.is_initial_registration = false;

                // TODO: this can most likely not happen at all, potentially remove these checks
                if rendezvous_node != self.rendezvous_point_peer_id {
                    tracing::error!(peer_id=%rendezvous_node, "Ignoring message from unknown rendezvous node");
                }

                // TODO: Consider implementing From for Namespace and XmrBtcNamespace
                if namespace.to_string() != self.rendezvous_namespace.to_string() {
                    tracing::error!(peer_id=%rendezvous_node, %namespace, "Ignoring message from rendezvous node for unknown namespace");
                }

                // record re-registration after half the ttl has expired
                self.rendezvous_reregister_timestamp =
                    Some(Instant::now() + Duration::from_secs(ttl) / 2);

                tracing::info!("Registration with rendezvous node successfull")
            }
            _ => {}
        }
    }
}
