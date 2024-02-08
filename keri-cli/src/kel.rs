use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::Arc,
};

use crate::init;
use ed25519_dalek::SigningKey;
use figment::{
    providers::{Format, Yaml},
    Figment,
};
use keri_controller::{BasicPrefix, CesrPrimitive, IdentifierPrefix, LocationScheme, SeedPrefix};
use serde::Deserialize;

use crate::{
    keri::{query_kel, rotate},
    utils::{load, load_next_signer, load_signer},
    CliError,
};

#[derive(Debug, Deserialize)]
struct RotationConfig {
    witness_to_add: Vec<LocationScheme>,
    witness_to_remove: Vec<BasicPrefix>,
    witness_threshold: u64,
    #[serde(deserialize_with = "init::deserialize_key")]
    new_next_seed: SeedPrefix,
    new_next_threshold: u64,
}

impl Default for RotationConfig {
    fn default() -> Self {
        let current = SigningKey::generate(&mut rand::rngs::OsRng);
        Self {
            witness_to_add: Default::default(),
            witness_to_remove: Default::default(),
            witness_threshold: 1,
            new_next_seed: SeedPrefix::RandomSeed256Ed25519(current.as_bytes().to_vec()),
            new_next_threshold: 1,
        }
    }
}

pub async fn handle_kel_query(alias: &str, about_who: &IdentifierPrefix) -> Result<String, CliError> {
    let id = load(alias).unwrap();
    let signer = Arc::new(load_signer(alias).unwrap());

    query_kel(about_who, &id, signer).await.map_err(|e| CliError::NotReady(e.to_string()))?;
    let kel = id.source.storage.get_kel(about_who).unwrap();
    kel.map(|kel| String::from_utf8(kel).unwrap()).ok_or(CliError::UnknownIdentifier(about_who.to_str()))
}

pub async fn handle_rotate(alias: &str, config_path: Option<PathBuf>) -> Result<(), CliError> {
    let rotation_config = match config_path {
        Some(cfgs) => Figment::new()
            .merge(Yaml::file(cfgs.clone()))
            .extract()
            .unwrap_or_else(|_| panic!("Can't read file from path: {:?}", cfgs.to_str())),
        None => RotationConfig::default(),
    };

    let id = load(alias).unwrap();
    // Load next keys as current
    let current_signer = Arc::new(load_next_signer(alias).unwrap());

    let (npk, _nsk) = rotation_config.new_next_seed.derive_key_pair().unwrap();
    let next_bp = BasicPrefix::Ed25519NT(npk);

    // Rotate keys
    rotate(
        &id,
        current_signer,
        vec![next_bp],
        rotation_config.new_next_threshold,
        rotation_config.witness_to_add,
        rotation_config.witness_to_remove,
        rotation_config.witness_threshold,
    )
    .await
    .unwrap();

    print!("\nKeys rotated for alias {} ({})", alias, id.id);

    // Save new settings in file
    let mut store_path = home::home_dir().unwrap();
    store_path.push(".keri-cli");
    store_path.push(&alias);

    let mut nsk_path = store_path.clone();
    nsk_path.push("next_priv_key");

    let mut priv_key_path = store_path.clone();
    priv_key_path.push("priv_key");

    fs::copy(&nsk_path, priv_key_path).unwrap();

    // Save new next key
    let mut file = File::create(nsk_path).unwrap();
    file.write_all(rotation_config.new_next_seed.to_str().as_bytes())
        .unwrap();

    Ok(())
}

pub async fn handle_get_kel(
    alias: &str,
    about_who: &IdentifierPrefix,
) -> Result<Option<String>, CliError> {
    let id = load(alias).unwrap();

    Ok(id
        .source
        .storage
        .get_kel(&about_who)
        .unwrap()
        .map(|v| String::from_utf8(v).unwrap()))
}
