use crate::error::Error;
use codec::{Decode, Encode};
use ipld_block_builder::{Cache, Codec};
use libipld::cbor::DagCborCodec;
use libipld::cid::Cid;
use libipld::codec::Codec as _;
use libipld::store::Store;
use libipld::DagCbor;
use std::time::{Duration, UNIX_EPOCH};
use substrate_subxt::sp_core::Pair;
use substrate_subxt::sp_runtime::traits::{IdentifyAccount, Verify};

#[derive(Clone, Debug, Eq, PartialEq, DagCbor)]
pub struct Claim {
    claim: UnsignedClaim,
    signature: Vec<u8>,
}

impl Claim {
    pub fn new<S, P>(pair: &P, claim: UnsignedClaim) -> Result<Self, Error>
    where
        S: Encode + Verify + From<P::Signature>,
        P: Pair,
    {
        let message = DagCborCodec::encode(&claim)?;
        let signature = Encode::encode(&S::from(pair.sign(&message)));
        Ok(Self { claim, signature })
    }

    pub fn verify<S>(
        &self,
        account_id: &<<S as Verify>::Signer as IdentifyAccount>::AccountId,
    ) -> Result<(), Error>
    where
        S: Encode + Decode + Verify,
        <S as Verify>::Signer: IdentifyAccount,
    {
        let message = DagCborCodec::encode(&self.claim)?;
        let signature: S =
            Decode::decode(&mut &self.signature[..]).map_err(|_| Error::InvalidSignature)?;
        if !signature.verify(&message[..], account_id) {
            return Err(Error::InvalidSignature);
        }
        if self.claim.expired() {
            return Err(Error::Expired);
        }
        Ok(())
    }

    pub fn body(&self) -> &ClaimBody {
        &self.claim.body
    }

    pub fn prev(&self) -> Option<&Cid> {
        self.claim.prev.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, DagCbor)]
pub struct UnsignedClaim {
    body: ClaimBody,
    prev: Option<Cid>,
    seqno: u32,
    ctime: u64,
    expire_in: u64,
}

impl UnsignedClaim {
    pub async fn new<S: Store>(
        cache: &mut Cache<S, Codec, Claim>,
        body: ClaimBody,
        prev: Option<Cid>,
        expire_in: Duration,
    ) -> Result<Self, Error> {
        let ctime = UNIX_EPOCH.elapsed().unwrap().as_millis() as u64;
        let expire_in = expire_in.as_millis() as u64;
        let seqno = if let Some(prev) = prev.as_ref() {
            cache.get(prev).await?.claim.seqno + 1
        } else {
            0
        };
        Ok(Self {
            body,
            prev,
            seqno,
            ctime,
            expire_in,
        })
    }

    pub fn expired(&self) -> bool {
        let expires_at = Duration::from_millis(self.ctime.saturating_add(self.expire_in));
        UNIX_EPOCH.elapsed().unwrap() > expires_at
    }
}

#[derive(Clone, Debug, Eq, PartialEq, DagCbor)]
pub enum ClaimBody {
    Github(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use libipld::mem::MemStore;
    use substrate_subxt::sp_core::ed25519::Pair;
    use substrate_subxt::sp_core::Pair as _;
    use substrate_subxt::sp_runtime::MultiSignature;

    #[async_std::test]
    async fn test_claims() {
        let pair = Pair::generate().0;
        let mut cache = Cache::new(MemStore::default(), Codec::new(), 2);

        let claim = UnsignedClaim::new(
            &mut cache,
            ClaimBody::Github("dvc94ch".into()),
            None,
            Duration::from_millis(0),
        )
        .await
        .unwrap();
        assert!(claim.expired());

        let claim = Claim::new::<MultiSignature, _>(&pair, claim).unwrap();
        assert!(claim
            .verify::<MultiSignature>(&pair.public().into())
            .is_err());

        let root = cache.insert(claim).await.unwrap();

        let claim = UnsignedClaim::new(
            &mut cache,
            ClaimBody::Github("dvc94ch".into()),
            Some(root),
            Duration::from_millis(u64::MAX),
        )
        .await
        .unwrap();
        assert!(!claim.expired());

        let claim = Claim::new::<MultiSignature, _>(&pair, claim).unwrap();
        assert!(claim
            .verify::<MultiSignature>(&pair.public().into())
            .is_ok());

        cache.insert(claim).await.unwrap();
    }
}
