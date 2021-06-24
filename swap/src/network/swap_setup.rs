use crate::monero;
use libp2p::core::upgrade;
use libp2p::swarm::NegotiatedSubstream;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const BUF_SIZE: usize = 1024 * 1024;

pub mod protocol {
    use futures::future;
    use libp2p::core::upgrade::{from_fn, FromFnUpgrade};
    use libp2p::core::Endpoint;
    use libp2p::swarm::NegotiatedSubstream;
    use void::Void;

    pub fn new() -> SwapSetup {
        from_fn(
            b"/comit/xmr/btc/swap_setup/1.0.0",
            Box::new(|socket, _| future::ready(Ok(socket))),
        )
    }

    pub type SwapSetup = FromFnUpgrade<
        &'static [u8],
        Box<
            dyn Fn(
                    NegotiatedSubstream,
                    Endpoint,
                ) -> future::Ready<Result<NegotiatedSubstream, Void>>
                + Send
                + 'static,
        >,
    >;
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct BlockchainNetwork {
    #[serde(with = "crate::bitcoin::network")]
    pub bitcoin: bitcoin::Network,
    #[serde(with = "crate::monero::network")]
    pub monero: monero::Network,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpotPriceRequest {
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc: bitcoin::Amount,
    pub blockchain_network: BlockchainNetwork,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SpotPriceResponse {
    Xmr(monero::Amount),
    Error(SpotPriceError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SpotPriceError {
    NoSwapsAccepted,
    AmountBelowMinimum {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        min: bitcoin::Amount,
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },
    AmountAboveMaximum {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        max: bitcoin::Amount,
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },
    BalanceTooLow {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },
    BlockchainNetworkMismatch {
        cli: BlockchainNetwork,
        asb: BlockchainNetwork,
    },
    /// To be used for errors that cannot be explained on the CLI side (e.g.
    /// rate update problems on the seller side)
    Other,
}

pub async fn read_cbor_message<T>(substream: &mut NegotiatedSubstream) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let bytes = upgrade::read_one(substream, BUF_SIZE).await?;
    let mut de = serde_cbor::Deserializer::from_slice(&bytes);
    let message = T::deserialize(&mut de)?;

    Ok(message)
}

pub async fn write_cbor_message<T>(
    substream: &mut NegotiatedSubstream,
    message: T,
) -> anyhow::Result<()>
where
    T: Serialize,
{
    let bytes = serde_cbor::to_vec(&message)?;
    upgrade::write_one(substream, &bytes).await?;

    Ok(())
}
