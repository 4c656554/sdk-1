use crate::config::dfinity::Config;
use crate::lib::builders::BuildConfig;
use crate::lib::canister_info::CanisterInfo;
use crate::lib::environment::Environment;
use crate::lib::error::DfxResult;
use crate::lib::ic_attributes::CanisterSettings;
use crate::lib::identity::identity_utils::CallSender;
use crate::lib::models::canister::CanisterPool;
use crate::lib::models::canister_id_store::CanisterIdStore;
use crate::lib::operations::canister::{create_canister, install_canister};
use crate::util::{blob_from_arguments, get_candid_init_type};

use anyhow::{anyhow, bail};
use humanize_rs::bytes::Bytes;
use ic_agent::AgentError;
use ic_types::Principal;
use ic_utils::interfaces::management_canister::attributes::{
    ComputeAllocation, FreezingThreshold, MemoryAllocation,
};
use ic_utils::interfaces::management_canister::builders::InstallMode;
use slog::info;
use std::convert::TryFrom;
use std::time::Duration;

#[allow(clippy::too_many_arguments)]
pub async fn deploy_canisters(
    env: &dyn Environment,
    some_canister: Option<&str>,
    argument: Option<&str>,
    argument_type: Option<&str>,
    timeout: Duration,
    with_cycles: Option<&str>,
    call_sender: &CallSender,
    effective_canister_id: Option<Principal>,
) -> DfxResult {
    let log = env.get_logger();

    let config = env
        .get_config()
        .ok_or_else(|| anyhow!("Cannot find dfx configuration file in the current working directory. Did you forget to create one?"))?;
    let initial_canister_id_store = CanisterIdStore::for_env(env)?;

    let canister_names = canisters_to_deploy(&config, some_canister)?;
    if some_canister.is_some() {
        info!(log, "Deploying: {}", canister_names.join(" "));
    } else {
        info!(log, "Deploying all canisters.");
    }

    register_canisters(
        env,
        &canister_names,
        &initial_canister_id_store,
        timeout,
        with_cycles,
        call_sender,
        &config,
        effective_canister_id.clone(),
    )
    .await?;

    build_canisters(env, &canister_names, &config)?;

    install_canisters(
        env,
        &canister_names,
        &initial_canister_id_store,
        &config,
        argument,
        argument_type,
        timeout,
        call_sender,
        effective_canister_id,
    )
    .await?;

    info!(log, "Deployed canisters.");

    Ok(())
}

fn canisters_to_deploy(config: &Config, some_canister: Option<&str>) -> DfxResult<Vec<String>> {
    let mut canister_names = config
        .get_config()
        .get_canister_names_with_dependencies(some_canister)?;
    canister_names.sort();
    Ok(canister_names)
}

async fn register_canisters(
    env: &dyn Environment,
    canister_names: &[String],
    canister_id_store: &CanisterIdStore,
    timeout: Duration,
    with_cycles: Option<&str>,
    call_sender: &CallSender,
    config: &Config,
    effective_canister_id: Option<Principal>,
) -> DfxResult {
    let canisters_to_create = canister_names
        .iter()
        .filter(|n| canister_id_store.find(&n).is_none())
        .cloned()
        .collect::<Vec<String>>();
    if canisters_to_create.is_empty() {
        info!(env.get_logger(), "All canisters have already been created.");
    } else {
        info!(env.get_logger(), "Creating canisters...");
        for canister_name in &canisters_to_create {
            let config_interface = config.get_config();
            let compute_allocation =
                config_interface
                    .get_compute_allocation(canister_name)?
                    .map(|arg| {
                        ComputeAllocation::try_from(arg.parse::<u64>().unwrap())
                            .expect("Compute Allocation must be a percentage.")
                    });
            let memory_allocation =
                config_interface
                    .get_memory_allocation(canister_name)?
                    .map(|arg| {
                        MemoryAllocation::try_from(
                        u64::try_from(arg.parse::<Bytes>().unwrap().size()).unwrap(),
                    )
                    .expect(
                        "Memory allocation must be between 0 and 2^48 (i.e 256TB), inclusively.",
                    )
                    });
            let freezing_threshold =
                config_interface
                    .get_freezing_threshold(canister_name)?
                    .map(|arg| {
                        FreezingThreshold::try_from(
                            u128::try_from(arg.parse::<Bytes>().unwrap().size()).unwrap(),
                        )
                        .expect("Freezing threshold must be between 0 and 2^64-1, inclusively.")
                    });
            let controller = None;
            create_canister(
                env,
                &canister_name,
                timeout,
                with_cycles,
                &call_sender,
                CanisterSettings {
                    controller,
                    compute_allocation,
                    memory_allocation,
                    freezing_threshold,
                },
                effective_canister_id.clone(),
            )
            .await?;
        }
    }
    Ok(())
}

fn build_canisters(env: &dyn Environment, canister_names: &[String], config: &Config) -> DfxResult {
    info!(env.get_logger(), "Building canisters...");
    let build_mode_check = false;
    let canister_pool = CanisterPool::load(env, build_mode_check, &canister_names)?;

    canister_pool.build_or_fail(BuildConfig::from_config(&config)?)
}

#[allow(clippy::too_many_arguments)]
async fn install_canisters(
    env: &dyn Environment,
    canister_names: &[String],
    initial_canister_id_store: &CanisterIdStore,
    config: &Config,
    argument: Option<&str>,
    argument_type: Option<&str>,
    timeout: Duration,
    call_sender: &CallSender,
    effective_canister_id: Option<Principal>,
) -> DfxResult {
    info!(env.get_logger(), "Installing canisters...");

    let agent = env
        .get_agent()
        .ok_or_else(|| anyhow!("Cannot find dfx configuration file in the current working directory. Did you forget to create one?"))?;

    let canister_id_store = CanisterIdStore::for_env(env)?;

    for canister_name in canister_names {
        let install_mode = match initial_canister_id_store.find(&canister_name) {
            Some(canister_id) => {
                match agent
                    .read_state_canister_info(canister_id, "module_hash")
                    .await
                {
                    Ok(_) => InstallMode::Upgrade,
                    // If the canister is empty, this path does not exist.
                    // The replica doesn't support negative lookups, therefore if the canister
                    // is empty, the replica will return lookup_path([], Pruned _) = Unknown
                    Err(AgentError::LookupPathUnknown(_))
                    | Err(AgentError::LookupPathAbsent(_)) => InstallMode::Install,
                    Err(x) => bail!(x),
                }
            }
            None => InstallMode::Install,
        };

        let canister_id = canister_id_store.get(&canister_name)?;
        let canister_info = CanisterInfo::load(&config, &canister_name, Some(canister_id))?;

        let maybe_path = canister_info.get_output_idl_path();
        let init_type = maybe_path.and_then(|path| get_candid_init_type(&path));
        let install_args = blob_from_arguments(argument, None, argument_type, &init_type)?;

        install_canister(
            env,
            &agent,
            &canister_info,
            &install_args,
            install_mode,
            timeout,
            &call_sender,
            effective_canister_id.clone(),
        )
        .await?;
    }

    Ok(())
}
