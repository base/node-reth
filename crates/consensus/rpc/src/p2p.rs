//! RPC Module to serve the P2P API.

use std::{net::IpAddr, str::FromStr, time::Duration};

use async_trait::async_trait;
use backon::{ExponentialBuilder, Retryable};
use base_consensus_gossip::{Metrics, P2pRpcRequest, PeerCount, PeerDump, PeerInfo, PeerStats};
use ipnet::IpNet;
use jsonrpsee::{
    core::RpcResult,
    types::{ErrorCode, ErrorObject},
};

use crate::{BaseP2PApiServer, net::P2pRpc};

const PEER_STATE_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

macro_rules! impl_p2p_api {
    (
        queries {
            $(fn $query:ident($($argument:ident: $argument_ty:ty),*) -> $query_ty:ty
                $(, $query_metric:literal)? =>
                |$tx:ident| $query_request:expr,
                |$response:ident| $result:expr;)*
        }
        commands {
            $(fn $command:ident($($command_arg:ident: $command_ty:ty),*),
                $command_metric:literal => $command_request:expr;)*
        }
        peers {
            $(fn $peer_command:ident($peer_arg:ident), $peer_metric:literal =>
                $peer_request:ident { $peer_field:ident };)*
        }
        $($extra:item)*
    ) => {
        #[async_trait]
        impl BaseP2PApiServer for P2pRpc {
            $(async fn $query(&self, $($argument: $argument_ty),*) -> RpcResult<$query_ty> {
                $(Metrics::rpc_calls($query_metric).increment(1.0);)?
                let (tx, rx) = tokio::sync::oneshot::channel();
                let $tx = tx;
                self.sender.send($query_request).await
                    .map_err(|_| ErrorObject::from(ErrorCode::InternalError))?;
                let $response = rx.await
                    .map_err(|_| ErrorObject::from(ErrorCode::InternalError))?;
                Ok($result)
            })*

            $(async fn $command(&self, $($command_arg: $command_ty),*) -> RpcResult<()> {
                Metrics::rpc_calls($command_metric).increment(1.0);
                self.sender.send($command_request).await
                    .map_err(|_| ErrorObject::from(ErrorCode::InternalError))
            })*

            $(async fn $peer_command(&self, $peer_arg: String) -> RpcResult<()> {
                Metrics::rpc_calls($peer_metric).increment(1.0);
                let peer_id = libp2p::PeerId::from_str(&$peer_arg)
                    .map_err(|_| ErrorObject::from(ErrorCode::InvalidParams))?;
                self.sender.send(P2pRpcRequest::$peer_request { $peer_field: peer_id }).await
                    .map_err(|_| ErrorObject::from(ErrorCode::InternalError))
            })*

            $($extra)*
        }
    };
}

impl_p2p_api! {
    queries {
        fn opp2p_self() -> PeerInfo, "opp2p_self" =>
            |tx| P2pRpcRequest::PeerInfo(tx), |response| response;
        fn opp2p_peer_count() -> PeerCount, "opp2p_peerCount" =>
            |tx| P2pRpcRequest::PeerCount(tx), |counts| PeerCount {
                connected_discovery: counts.0,
                connected_gossip: counts.1,
            };
        fn opp2p_peers(connected: bool) -> PeerDump, "opp2p_peers" =>
            |tx| P2pRpcRequest::Peers { out: tx, connected }, |response| response;
        fn opp2p_peer_stats() -> PeerStats =>
            |tx| P2pRpcRequest::PeerStats(tx), |response| response;
        fn opp2p_discovery_table() -> Vec<String>, "opp2p_discoveryTable" =>
            |tx| P2pRpcRequest::DiscoveryTable(tx), |response| response;
        fn opp2p_list_blocked_peers() -> Vec<String>, "opp2p_listBlockedPeers" =>
            |tx| P2pRpcRequest::ListBlockedPeers(tx),
            |peers| peers.iter().map(ToString::to_string).collect();
        fn opp2p_list_blocked_addrs() -> Vec<IpAddr>, "opp2p_listBlockedAddrs" =>
            |tx| P2pRpcRequest::ListBlockedAddrs(tx), |response| response;
        fn opp2p_list_blocked_subnets() -> Vec<IpNet>, "opp2p_listBlockedSubnets" =>
            |tx| P2pRpcRequest::ListBlockedSubnets(tx), |response| response;
    }
    commands {
        fn opp2p_block_addr(address: IpAddr), "opp2p_blockAddr" =>
            P2pRpcRequest::BlockAddr { address };
        fn opp2p_unblock_addr(address: IpAddr), "opp2p_unblockAddr" =>
            P2pRpcRequest::UnblockAddr { address };
        fn opp2p_block_subnet(subnet: IpNet), "opp2p_blockSubnet" =>
            P2pRpcRequest::BlockSubnet { address: subnet };
        fn opp2p_unblock_subnet(subnet: IpNet), "opp2p_unblockSubnet" =>
            P2pRpcRequest::UnblockSubnet { address: subnet };
    }
    peers {
        fn opp2p_block_peer(peer_id), "opp2p_blockPeer" => BlockPeer { id };
        fn opp2p_unblock_peer(peer_id), "opp2p_unblockPeer" => UnblockPeer { id };
        fn opp2p_protect_peer(id), "opp2p_protectPeer" => ProtectPeer { peer_id };
        fn opp2p_unprotect_peer(id), "opp2p_unprotectPeer" => UnprotectPeer { peer_id };
    }

    async fn opp2p_connect_peer(&self, peer: String) -> RpcResult<()> {
        Metrics::rpc_calls("opp2p_connectPeer").increment(1.0);
        self.connect_peer_with_backoff(
            peer,
            ExponentialBuilder::default().with_total_delay(Some(PEER_STATE_WAIT_TIMEOUT)),
        )
        .await
    }

    async fn opp2p_disconnect_peer(&self, peer_id: String) -> RpcResult<()> {
        Metrics::rpc_calls("opp2p_disconnectPeer").increment(1.0);
        self.disconnect_peer_with_backoff(
            peer_id,
            ExponentialBuilder::default().with_total_delay(Some(PEER_STATE_WAIT_TIMEOUT)),
        )
        .await
    }
}

impl P2pRpc {
    async fn connect_peer_with_backoff(
        &self,
        peer: String,
        backoff: ExponentialBuilder,
    ) -> RpcResult<()> {
        let ma = libp2p::Multiaddr::from_str(&peer).map_err(|_| {
            ErrorObject::borrowed(ErrorCode::InvalidParams.code(), "Invalid multiaddr", None)
        })?;

        let peer_id = ma
            .iter()
            .find_map(|component| match component {
                libp2p::multiaddr::Protocol::P2p(peer_id) => Some(peer_id),
                _ => None,
            })
            .ok_or_else(|| {
                ErrorObject::borrowed(
                    ErrorCode::InvalidParams.code(),
                    "Impossible to extract peer ID from multiaddr",
                    None,
                )
            })?;

        self.sender.send(P2pRpcRequest::ConnectPeer { address: ma }).await.map_err(|_| {
            ErrorObject::borrowed(
                ErrorCode::InternalError.code(),
                "Failed to send connect peer request",
                None,
            )
        })?;

        // We need to wait until both peers are connected to each other to return from this method.
        // We try with an exponential backoff and return an error if we fail to connect to the peer.
        let is_connected = async || {
            let (tx, rx) = tokio::sync::oneshot::channel();

            self.sender
                .send(P2pRpcRequest::Peers { out: tx, connected: true })
                .await
                .map_err(|_| ErrorObject::from(ErrorCode::InternalError))?;

            let peers = rx.await.map_err(|_| {
                ErrorObject::borrowed(ErrorCode::InternalError.code(), "Failed to get peers", None)
            })?;

            // InvalidParams = "not connected yet" (retryable). InternalError = channel failure
            // (fail fast via .when() below).
            if peers.peers.contains_key(&peer_id.to_string()) {
                Ok(())
            } else {
                Err(ErrorObject::borrowed(
                    ErrorCode::InvalidParams.code(),
                    "Peer not connected",
                    None,
                ))
            }
        };

        // Retry only peer-state misses; do not retry channel failures.
        is_connected
            .retry(backoff)
            .when(|error| error.code() == ErrorCode::InvalidParams.code())
            .await?;
        Ok(())
    }

    async fn disconnect_peer_with_backoff(
        &self,
        peer_id: String,
        backoff: ExponentialBuilder,
    ) -> RpcResult<()> {
        let peer_id = match peer_id.parse() {
            Ok(id) => id,
            Err(err) => {
                warn!(target: "rpc", ?err, ?peer_id, "Failed to parse peer ID");
                return Err(ErrorObject::from(ErrorCode::InvalidParams));
            }
        };

        self.sender
            .send(P2pRpcRequest::DisconnectPeer { peer_id })
            .await
            .map_err(|_| ErrorObject::from(ErrorCode::InternalError))?;

        // We need to wait until both peers are fully disconnected to each other to return from this
        // method. We try with an exponential backoff and return an error if we fail to
        // disconnect from the peer.
        let is_not_connected = async || {
            let (tx, rx) = tokio::sync::oneshot::channel();

            self.sender
                .send(P2pRpcRequest::Peers { out: tx, connected: true })
                .await
                .map_err(|_| ErrorObject::from(ErrorCode::InternalError))?;

            let peers = rx.await.map_err(|_| {
                ErrorObject::borrowed(ErrorCode::InternalError.code(), "Failed to get peers", None)
            })?;

            // InvalidParams = "still connected" (retryable). InternalError = channel failure
            // (fail fast via .when() below).
            if peers.peers.contains_key(&peer_id.to_string()) {
                Err(ErrorObject::borrowed(
                    ErrorCode::InvalidParams.code(),
                    "Peers are still connected",
                    None,
                ))
            } else {
                Ok(())
            }
        };

        // Retry only peer-state misses; do not retry channel failures.
        is_not_connected
            .retry(backoff)
            .when(|error| error.code() == ErrorCode::InvalidParams.code())
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        str::FromStr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use backon::ExponentialBuilder;
    use base_consensus_gossip::{P2pRpcRequest, PeerDump, PeerInfo};
    use tokio::sync::mpsc;

    use crate::{BaseP2PApiServer, net::P2pRpc};

    fn test_backoff() -> ExponentialBuilder {
        ExponentialBuilder::default()
            .with_min_delay(Duration::from_millis(1))
            .with_total_delay(Some(Duration::from_millis(20)))
    }

    async fn respond_to_peer_state_requests(
        mut requests: mpsc::Receiver<P2pRpcRequest>,
        peer_id: libp2p::PeerId,
        mut connected_states: VecDeque<bool>,
        default_connected: bool,
        attempts: Arc<AtomicUsize>,
    ) {
        while let Some(request) = requests.recv().await {
            match request {
                P2pRpcRequest::ConnectPeer { .. } | P2pRpcRequest::DisconnectPeer { .. } => {}
                P2pRpcRequest::Peers { out, connected: true } => {
                    attempts.fetch_add(1, Ordering::Relaxed);
                    let connected = connected_states.pop_front().unwrap_or(default_connected);
                    let mut dump = PeerDump::default();
                    if connected {
                        dump.total_connected = 1;
                        dump.peers.insert(peer_id.to_string(), PeerInfo::default());
                    }
                    let _ = out.send(dump);
                }
                _ => panic!("unexpected p2p request"),
            }
        }
    }

    fn peer_multiaddr(peer_id: &libp2p::PeerId) -> String {
        format!("/ip4/127.0.0.1/tcp/30303/p2p/{peer_id}")
    }

    #[tokio::test]
    async fn malformed_peer_id_is_invalid_params() {
        let (sender, _requests) = mpsc::channel(1);
        let error = P2pRpc::new(sender)
            .opp2p_block_peer("not-a-peer-id".to_string())
            .await
            .expect_err("a malformed peer ID should be rejected");

        assert_eq!(error.code(), jsonrpsee::types::ErrorCode::InvalidParams.code());
    }

    #[tokio::test]
    async fn closed_request_channel_is_internal_error() {
        let (sender, requests) = mpsc::channel(1);
        drop(requests);

        let error = P2pRpc::new(sender)
            .opp2p_discovery_table()
            .await
            .expect_err("a closed request channel should fail");

        assert_eq!(error.code(), jsonrpsee::types::ErrorCode::InternalError.code());
    }

    #[tokio::test]
    async fn dropped_query_reply_is_internal_error() {
        let (sender, mut requests) = mpsc::channel(1);
        let rpc = P2pRpc::new(sender);
        let call = tokio::spawn(async move { rpc.opp2p_self().await });
        let P2pRpcRequest::PeerInfo(reply) = requests.recv().await.unwrap() else {
            panic!("unexpected p2p request");
        };
        drop(reply);

        let error = call.await.unwrap().expect_err("a dropped reply should fail");
        assert_eq!(error.code(), jsonrpsee::types::ErrorCode::InternalError.code());
    }

    #[tokio::test]
    async fn valid_address_argument_is_dispatched() {
        let (sender, mut requests) = mpsc::channel(1);
        let rpc = P2pRpc::new(sender);
        let address = "192.0.2.1".parse().unwrap();

        rpc.opp2p_block_addr(address).await.unwrap();

        let P2pRpcRequest::BlockAddr { address: dispatched } = requests.recv().await.unwrap()
        else {
            panic!("unexpected p2p request");
        };
        assert_eq!(dispatched, address);
    }

    #[tokio::test]
    async fn connect_peer_waits_for_multiple_peer_state_responses() {
        let (sender, requests) = mpsc::channel(8);
        let rpc = P2pRpc::new(sender);
        let peer_id = libp2p::PeerId::random();
        let attempts = Arc::new(AtomicUsize::new(0));
        let handler = tokio::spawn(respond_to_peer_state_requests(
            requests,
            peer_id,
            VecDeque::from([false, false, true]),
            false,
            Arc::clone(&attempts),
        ));

        let result = rpc.connect_peer_with_backoff(peer_multiaddr(&peer_id), test_backoff()).await;

        assert!(result.is_ok());
        assert_eq!(attempts.load(Ordering::Relaxed), 3);
        drop(rpc);
        handler.await.unwrap();
    }

    #[tokio::test]
    async fn disconnect_peer_waits_for_multiple_peer_state_responses() {
        let (sender, requests) = mpsc::channel(8);
        let rpc = P2pRpc::new(sender);
        let peer_id = libp2p::PeerId::random();
        let attempts = Arc::new(AtomicUsize::new(0));
        let handler = tokio::spawn(respond_to_peer_state_requests(
            requests,
            peer_id,
            VecDeque::from([true, true, false]),
            true,
            Arc::clone(&attempts),
        ));

        let result = rpc.disconnect_peer_with_backoff(peer_id.to_string(), test_backoff()).await;

        assert!(result.is_ok());
        assert_eq!(attempts.load(Ordering::Relaxed), 3);
        drop(rpc);
        handler.await.unwrap();
    }

    #[tokio::test]
    async fn connect_peer_returns_error_after_timeout() {
        let (sender, requests) = mpsc::channel(8);
        let rpc = P2pRpc::new(sender);
        let peer_id = libp2p::PeerId::random();
        let attempts = Arc::new(AtomicUsize::new(0));
        let handler = tokio::spawn(respond_to_peer_state_requests(
            requests,
            peer_id,
            VecDeque::new(),
            false,
            Arc::clone(&attempts),
        ));

        let result = rpc.connect_peer_with_backoff(peer_multiaddr(&peer_id), test_backoff()).await;

        let error = result.expect_err("an absent peer should eventually time out");
        assert_eq!(error.message(), "Peer not connected");
        assert!(attempts.load(Ordering::Relaxed) > 1);
        drop(rpc);
        handler.await.unwrap();
    }

    #[test]
    fn test_parse_multiaddr_string() {
        let ma = "/ip4/127.0.0.1/udt";
        let multiaddr = libp2p::Multiaddr::from_str(ma).unwrap();
        let components = multiaddr.iter().collect::<Vec<_>>();
        assert_eq!(
            components[0],
            libp2p::multiaddr::Protocol::Ip4(std::net::Ipv4Addr::new(127, 0, 0, 1))
        );
        assert_eq!(components[1], libp2p::multiaddr::Protocol::Udt);
    }
}
