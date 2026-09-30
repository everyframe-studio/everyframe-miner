use crate::{Error, Result, need, s};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::{RngCore, rngs::OsRng};
use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha512};
use x25519_dalek::{PublicKey, StaticSecret};
const ED: &[u8] = &[0x30, 0x2a, 0x30, 5, 6, 3, 0x2b, 0x65, 0x70, 3, 0x21, 0];
const X: &[u8] = &[0x30, 0x2a, 0x30, 5, 6, 3, 0x2b, 0x65, 0x6e, 3, 0x21, 0];
pub fn b64(b: &[u8]) -> String {
    STANDARD.encode(b)
}
pub fn decode(s: &str) -> Result<Vec<u8>> {
    STANDARD.decode(s).map_err(|_| Error("invalid_base64"))
}
pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0; N];
    OsRng.fill_bytes(&mut b);
    b
}
pub fn id() -> String {
    hex::encode(random::<24>())
}
pub fn sha(b: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(b))
}
pub fn canonical(v: &Value) -> Result<String> {
    match v {
        Value::Number(_) => {
            crate::n(v)?;
            Ok(v.to_string())
        }
        Value::Array(a) => Ok(format!(
            "[{}]",
            a.iter()
                .map(canonical)
                .collect::<Result<Vec<_>>>()?
                .join(",")
        )),
        Value::Object(o) => {
            let mut keys = o.keys().collect::<Vec<_>>();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            Ok(format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|k| Ok(format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap(),
                        canonical(&o[k])?
                    )))
                    .collect::<Result<Vec<_>>>()?
                    .join(",")
            ))
        }
        _ => Ok(v.to_string()),
    }
}
pub fn digest(v: &Value) -> Result<String> {
    Ok(sha(canonical(v)?))
}
pub fn exact(v: &Value, keys: &[&str]) -> Result<()> {
    let o = v.as_object().ok_or(Error("invalid_fields"))?;
    need(
        o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)),
        "invalid_fields",
    )
}
pub fn export(raw: &[u8; 32], ed: bool) -> String {
    b64(&[if ed { ED } else { X }, raw].concat())
}
pub fn public(raw: &str, ed: bool) -> Result<[u8; 32]> {
    let b = decode(raw)?;
    need(
        b.len() == 44 && b[..12] == *if ed { ED } else { X },
        "invalid_key",
    )?;
    b[12..].try_into().map_err(|_| Error("invalid_key"))
}
pub struct Keys {
    pub signing: SigningKey,
    pub encryption: StaticSecret,
}
impl Default for Keys {
    fn default() -> Self {
        Self {
            signing: SigningKey::generate(&mut OsRng),
            encryption: StaticSecret::random_from_rng(OsRng),
        }
    }
}
impl Keys {
    pub fn signing_key(&self) -> String {
        export(self.signing.verifying_key().as_bytes(), true)
    }
    pub fn encryption_key(&self) -> String {
        export(PublicKey::from(&self.encryption).as_bytes(), false)
    }
}
pub fn signed(v: &Value, key: &SigningKey) -> Result<Value> {
    Ok(json!({"value":v,"signature":b64(&key.sign(canonical(v)?.as_bytes()).to_bytes())}))
}
pub fn verified(envelope: &Value, key: &str) -> Result<Value> {
    exact(envelope, &["value", "signature"])?;
    let sig = s(&envelope["signature"])?;
    need(sig.len() == 88, "invalid_signature")?;
    let sig = Signature::from_slice(&decode(sig)?).map_err(|_| Error("invalid_signature"))?;
    VerifyingKey::from_bytes(&public(key, true)?)
        .map_err(|_| Error("invalid_key"))?
        .verify_strict(canonical(&envelope["value"])?.as_bytes(), &sig)
        .map_err(|_| Error("invalid_signature"))?;
    Ok(envelope["value"].clone())
}
pub fn report_data(v: &Value) -> Result<Vec<u8>> {
    let mut h = Sha512::new();
    h.update(b"everyframe-attestation-v1\0");
    h.update(canonical(v)?);
    Ok(h.finalize().to_vec())
}
fn cipher_key(secret: &StaticSecret, peer: &str, context: &str) -> Result<[u8; 32]> {
    let shared = secret.diffie_hellman(&PublicKey::from(public(peer, false)?));
    need(shared.was_contributory(), "invalid_key")?;
    let mut key = [0; 32];
    hkdf::Hkdf::<Sha256>::new(Some(&[0; 32]), shared.as_bytes())
        .expand(format!("everyframe-job-v1:{context}").as_bytes(), &mut key)
        .map_err(|_| Error("invalid_key"))?;
    Ok(key)
}
pub fn seal(v: &Value, recipient: &str, context: &str) -> Result<Value> {
    let secret = StaticSecret::random_from_rng(OsRng);
    let iv = random::<12>();
    let key = cipher_key(&secret, recipient, context)?;
    let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
    let mut data = cipher
        .encrypt(
            Nonce::from_slice(&iv),
            Payload {
                msg: canonical(v)?.as_bytes(),
                aad: context.as_bytes(),
            },
        )
        .map_err(|_| Error("invalid_ciphertext"))?;
    let tag = data.split_off(data.len() - 16);
    Ok(
        json!({"ephemeral":export(PublicKey::from(&secret).as_bytes(),false),"iv":b64(&iv),"data":b64(&data),"tag":b64(&tag)}),
    )
}
pub fn unseal(p: &Value, secret: &StaticSecret, context: &str) -> Result<Value> {
    exact(p, &["ephemeral", "iv", "data", "tag"])?;
    let iv = decode(s(&p["iv"])?)?;
    let tag = decode(s(&p["tag"])?)?;
    need(iv.len() == 12 && tag.len() == 16, "invalid_ciphertext")?;
    let key = cipher_key(secret, s(&p["ephemeral"])?, context)?;
    let data = [decode(s(&p["data"])?)?, tag].concat();
    let clear = Aes256Gcm::new_from_slice(&key)
        .unwrap()
        .decrypt(
            Nonce::from_slice(&iv),
            Payload {
                msg: &data,
                aad: context.as_bytes(),
            },
        )
        .map_err(|_| Error("invalid_ciphertext"))?;
    serde_json::from_slice(&clear).map_err(|_| Error("invalid_plaintext"))
}
