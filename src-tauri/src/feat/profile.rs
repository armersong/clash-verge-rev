use crate::core::notify::NoticeStatus;
use crate::core::notify::{Refresh, announce};
use crate::{
    cmd,
    config::{Config, PrfItem, PrfOption, profiles::profiles_update_item_safe},
    core::{CoreManager, handle, tray, validate::ValidationOutcome},
    process::AsyncHandler,
    utils::help::{mask_err, mask_url},
};
use anyhow::{Result, bail};
use clash_verge_logging::{Type, logging};
use smartstring::alias::String;
use tauri_plugin_mihomo::models::Proxies;

/// Toggle proxy profile
pub async fn toggle_proxy_profile(profile_index: String) {
    if let Err(err) = cmd::patch_profiles_config_by_profile_index(profile_index.clone()).await {
        logging!(
            error,
            Type::Config,
            "toggle proxy profile {profile_index} failed: {err}"
        );
    }
}

pub async fn switch_proxy_node(group_name: &str, proxy_name: &str, unfix: bool) -> Result<()> {
    let mihomo = handle::Handle::mihomo();
    let previous_proxy = if Config::verge().await.data_arc().auto_close_connection.unwrap_or(false) {
        match mihomo.get_proxy_by_name(group_name).await {
            Ok(group) => group.now.filter(|name| !name.is_empty()),
            Err(err) => {
                logging!(
                    warn,
                    Type::Core,
                    "Failed to read previous proxy selection for {group_name}: {err}"
                );
                None
            }
        }
    } else {
        None
    };
    if unfix {
        mihomo.unfixed_proxy(group_name).await?;
    } else {
        mihomo.select_node_for_group(group_name, proxy_name).await?;
    }

    let save_result = if unfix {
        crate::config::profiles::forget_selected_node(group_name).await
    } else {
        crate::config::profiles::record_selected_node(group_name, proxy_name).await
    };
    // The core selection is already applied; persistence failure must not report it as failed.
    if let Err(error) = save_result {
        logging!(
            warn,
            Type::Config,
            "Failed to persist proxy selection for {group_name}: {error:#}"
        );
    }
    announce(Refresh::Proxies);

    if let Some(previous_proxy) = previous_proxy {
        AsyncHandler::spawn(move || async move {
            let connections = match mihomo.get_connections().await {
                Ok(connections) => connections,
                Err(err) => {
                    logging!(
                        warn,
                        Type::Core,
                        "Failed to read connections after switching proxy: {err}"
                    );
                    return;
                }
            };
            futures::future::join_all(
                connections
                    .connections
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|connection| connection.chains.contains(&previous_proxy))
                    .map(|connection| async move {
                        if let Err(err) = mihomo.close_connection(&connection.id).await {
                            logging!(
                                warn,
                                Type::Core,
                                "Failed to close connection {} after switching proxy: {err}",
                                connection.id
                            );
                        }
                    }),
            )
            .await;
        });
    }
    if let Err(err) = tray::Tray::global().update_menu().await {
        logging!(
            warn,
            Type::Tray,
            "Failed to refresh tray after switching proxy: {err:#}"
        );
    }
    Ok(())
}

async fn should_update_profile(uid: &String, ignore_auto_update: bool) -> Result<Option<(String, Option<PrfOption>)>> {
    let profiles = Config::profiles().await;
    let profiles = profiles.latest_arc();
    let item = profiles.get_item(uid)?;
    let is_remote = item.itype.as_ref().is_some_and(|s| s == "remote");

    if !is_remote {
        logging!(info, Type::Config, "[订阅更新] 不是远程订阅，跳过更新");
        Ok(None)
    } else if item.url.is_none() {
        logging!(warn, Type::Config, "[订阅更新] 缺少URL，无法更新");
        bail!("failed to get the profile item url");
    } else if !ignore_auto_update && !item.option.as_ref().and_then(|o| o.allow_auto_update).unwrap_or(true) {
        logging!(info, Type::Config, "[订阅更新] 禁止自动更新，跳过更新");
        Ok(None)
    } else {
        logging!(
            info,
            Type::Config,
            "[订阅更新] 远程订阅，URL: {}",
            mask_url(
                item.url
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Profile URL is None"))?
            )
        );
        Ok(Some((
            item.url.clone().ok_or_else(|| anyhow::anyhow!("Profile URL is None"))?,
            item.option.clone(),
        )))
    }
}

#[tracing::instrument(skip_all, level = "info", fields(uid = %uid, strategy = tracing::field::Empty, configured_elapsed = tracing::field::Empty, clash_elapsed = tracing::field::Empty, system_elapsed = tracing::field::Empty))]
async fn perform_profile_update(
    uid: &String,
    url: &String,
    opt: Option<&PrfOption>,
    option: Option<&PrfOption>,
    is_mannual_trigger: bool,
) -> Result<()> {
    let merged_opt = PrfOption::merge(opt, option);
    let profiles = Config::profiles().await;
    let profiles_arc = profiles.latest_arc();
    let profile_name = profiles_arc
        .get_item(uid)
        .ok()
        .and_then(|item| item.name.clone())
        .unwrap_or_else(|| String::from("UnKnown Profile"));

    #[derive(Clone, Copy)]
    enum Strategy {
        Configured,
        Clash,
        System,
    }

    let result =
        crate::utils::retry::try_strategies(Strategy::Configured, [Strategy::Clash, Strategy::System], |strategy| {
            let mut options = merged_opt.clone();
            async move {
                let (name, success, failure) = match strategy {
                    Strategy::Configured => (
                        "configured",
                        "[订阅更新] 更新订阅配置成功",
                        "[订阅更新] 直接更新失败: {}，尝试使用Clash代理更新",
                    ),
                    Strategy::Clash => (
                        "clash",
                        "[订阅更新] 使用 Clash代理 更新订阅配置成功",
                        "[订阅更新] Clash代理更新失败: {}，尝试使用系统代理更新",
                    ),
                    Strategy::System => (
                        "system",
                        "[订阅更新] 使用 系统代理 更新订阅配置成功",
                        "[订阅更新] 系统代理更新失败: {}，所有重试均已失败",
                    ),
                };
                match strategy {
                    Strategy::Configured => {}
                    Strategy::Clash | Strategy::System => {
                        let option = options.get_or_insert_with(PrfOption::default);
                        option.self_proxy = Some(matches!(strategy, Strategy::Clash));
                        option.with_proxy = Some(matches!(strategy, Strategy::System));
                    }
                }
                tracing::Span::current().record("strategy", name);
                let started = std::time::Instant::now();
                let result = PrfItem::from_url(url, None, None, options.as_ref()).await;
                let elapsed_field = match strategy {
                    Strategy::Configured => "configured_elapsed",
                    Strategy::Clash => "clash_elapsed",
                    Strategy::System => "system_elapsed",
                };
                tracing::Span::current().record(elapsed_field, tracing::field::debug(started.elapsed()));
                match &result {
                    Ok(_) => logging!(info, Type::Config, "{success}"),
                    Err(error) => logging!(
                        warn,
                        Type::Config,
                        "{}",
                        failure.replace("{}", &mask_err(&format!("{error:#}")))
                    ),
                }
                result
            }
        })
        .await;
    let last_err = match result {
        Ok((strategy, mut item)) => {
            // A persistence error must not replay a successful download with another transport.
            profiles_update_item_safe(uid, &mut item).await?;
            if !matches!(strategy, Strategy::Configured) {
                handle::Handle::notice(NoticeStatus::UpdateWithClashProxy, profile_name.as_str());
            }
            return Ok(());
        }
        Err(error) => error,
    };

    let last_err = mask_err(&last_err.to_string());
    if is_mannual_trigger {
        handle::Handle::notice(
            NoticeStatus::UpdateFailedEvenWithClash,
            format!("{profile_name} - {last_err}"),
        );
    }
    bail!(last_err)
}

#[tracing::instrument(skip_all, level = "info", fields(uid = %uid, manual = is_mannual_trigger))]
pub async fn update_profile(uid: &String, option: Option<&PrfOption>, is_mannual_trigger: bool) -> Result<()> {
    let url_opt = should_update_profile(uid, is_mannual_trigger).await?;

    let profile_persisted = match url_opt {
        Some((url, opt)) => {
            perform_profile_update(uid, &url, opt.as_ref(), option, is_mannual_trigger).await?;
            true
        }
        None => false,
    };
    let profiles = Config::profiles().await;
    let is_current = profiles.latest_arc().current.as_ref() == Some(uid);
    let should_refresh = is_current || (!profile_persisted && is_mannual_trigger);

    if should_refresh {
        logging!(debug, Type::Config, "[订阅更新] 更新内核配置");
        match CoreManager::global().update_config_with_force(is_mannual_trigger).await {
            Ok(outcome) if outcome.is_valid() => announce(Refresh::Clash),
            Ok(outcome @ (ValidationOutcome::Skipped { .. } | ValidationOutcome::Busy)) if !is_mannual_trigger => {
                logging!(info, Type::Config, "[订阅更新] 本次配置刷新已跳过: {}", outcome);
            }
            result => {
                let message = match result {
                    Ok(outcome) => outcome.to_string(),
                    Err(err) => err.to_string(),
                };
                let message = mask_err(&message);
                let message = if profile_persisted {
                    format!("订阅已保存，但未能确认已应用到内核: {message}")
                } else {
                    message
                };
                logging!(error, Type::Config, "[订阅更新] 更新失败: {}", message);
                handle::Handle::notice(NoticeStatus::UpdateFailed, message.as_str());
                bail!(message);
            }
        }
    }

    Ok(())
}

/// 增强配置
pub async fn enhance_profiles() -> Result<ValidationOutcome> {
    CoreManager::global().update_config_forced().await
}

/// 刷新所有远程订阅的服务器列表。
///
/// 与 `update_profile` 不同：只重新拉取并保存订阅内容，不触发内核配置重载。
/// 返回 `Ok(true)` 表示确实存在远程订阅但全部刷新失败（调用方据此挂 1 分钟重试任务）。
pub async fn refresh_all_remote_subscriptions() -> Result<bool> {
    logging!(info, Type::Config, "[订阅刷新] 开始刷新所有远程订阅");

    let targets: Vec<(String, String, Option<PrfOption>)> = {
        let profiles = Config::profiles().await;
        let data = profiles.latest_arc();
        data.items
            .as_ref()
            .map(|items| {
                items
                    .iter()
                    .filter(|item| item.itype.as_ref().is_some_and(|itype| itype == "remote"))
                    .filter_map(|item| {
                        let uid = item.uid.clone()?;
                        let url = item.url.clone()?;
                        Some((uid, url, item.option.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };

    if targets.is_empty() {
        logging!(info, Type::Config, "[订阅刷新] 没有远程订阅，跳过");
        return Ok(false);
    }

    let mut refreshed_count = 0usize;
    let mut failed_count = 0usize;

    for (uid, url, opt) in targets {
        logging!(info, Type::Config, "[订阅刷新] 刷新订阅: {}", uid);

        match perform_profile_update(&uid, &url, opt.as_ref(), None, false).await {
            Ok(()) => {
                refreshed_count += 1;
                logging!(info, Type::Config, "[订阅刷新] 订阅刷新成功: {}", uid);
                handle::Handle::notify_profile_update_completed(&uid);
            }
            Err(e) => {
                failed_count += 1;
                logging!(error, Type::Config, "[订阅刷新] 订阅刷新失败 {}: {e:#}", uid);
            }
        }
    }

    logging!(
        info,
        Type::Config,
        "[订阅刷新] 完成: 成功 {} 个, 失败 {} 个",
        refreshed_count,
        failed_count
    );

    Ok(refreshed_count == 0 && failed_count > 0)
}

/// 自动选择最佳代理节点
/// 遍历所有代理组，选择延迟最低的节点
pub async fn auto_select_best_proxies() -> Result<()> {
    logging!(info, Type::Config, "[自动选优] 开始自动选择最佳代理节点");

    let proxies_data: Proxies = match handle::Handle::mihomo().get_proxies().await {
        Ok(p) => p,
        Err(e) => {
            logging!(error, Type::Config, "[自动选优] 获取代理列表失败: {}", e);
            return Ok(());
        }
    };

    let mut switched_count = 0;

    // Iterate through all proxy groups
    for (group_name, group_data) in proxies_data.proxies.iter() {
        // Skip groups with no proxies
        let Some(all_proxies) = group_data.all.as_ref() else {
            continue;
        };

        if all_proxies.len() < 2 {
            // Need at least 2 proxies to make a selection
            continue;
        }

        let now_proxy = group_data.now.as_deref().unwrap_or_default();

        // Find the best proxy by checking delay history
        let best_proxy = find_best_proxy_from_history(&proxies_data, all_proxies, now_proxy);

        if let Some(best) = best_proxy {
            if best != now_proxy {
                logging!(
                    info,
                    Type::Config,
                    "[自动选优] 组 {} 切换: {} -> {}",
                    group_name,
                    now_proxy,
                    best
                );
                switch_proxy_node(group_name, &best).await;
                switched_count += 1;
            }
        }
    }

    logging!(info, Type::Config, "[自动选优] 完成: 切换了 {} 个组", switched_count);

    Ok(())
}

/// 从延迟历史记录中找到最佳代理
/// 规则：
/// 1. 如果当前服务器在搜索结果中且延迟小于500ms，不切换
/// 2. 只有当前服务器不在结果中或延迟>=500ms时才切换到延迟更低的服务器
fn find_best_proxy_from_history<'a>(
    proxies_data: &'a Proxies,
    all_proxies: &'a [std::string::String],
    current_proxy: &str,
) -> Option<std::string::String> {
    const KEEP_CURRENT_DELAY_THRESHOLD_MS: u16 = 500;

    // First, check if current proxy is still valid (exists in results and delay < 500ms)
    if let Some(current_info) = proxies_data.proxies.get(current_proxy) {
        if let Some(history) = current_info.history.last() {
            if history.delay > 0 && history.delay < KEEP_CURRENT_DELAY_THRESHOLD_MS {
                // Current proxy is working well, keep it
                logging!(
                    debug,
                    Type::Config,
                    "[自动选优] 当前服务器 {} 延迟 {}ms < 500ms，保持不变",
                    current_proxy,
                    history.delay
                );
                return None;
            }
        }
    }

    // Current proxy is not available or delay >= 500ms, find the best one
    let mut best_proxy: Option<(std::string::String, u16)> = None;

    for proxy_name in all_proxies {
        // Get the proxy's delay history
        if let Some(proxy_info) = proxies_data.proxies.get(proxy_name) {
            if let Some(history) = proxy_info.history.last() {
                // delay of 0 means timeout/unavailable, skip it
                if history.delay == 0 || history.delay >= 10000 {
                    continue;
                }

                match &best_proxy {
                    None => best_proxy = Some((proxy_name.clone(), history.delay)),
                    Some((_, best_delay)) if history.delay < *best_delay => {
                        best_proxy = Some((proxy_name.clone(), history.delay));
                    }
                    _ => {}
                }
            }
        }
    }

    // If we found a best proxy with valid delay, return it
    if let Some((name, delay)) = best_proxy {
        logging!(
            debug,
            Type::Config,
            "[自动选优] 找到最佳代理: {} (延迟 {}ms)",
            name,
            delay
        );
        return Some(name);
    }

    // Fallback: if no delay history, return current proxy to avoid unnecessary switching
    logging!(
        warn,
        Type::Config,
        "[自动选优] 没有找到有效的延迟历史，使用当前代理: {}",
        current_proxy
    );
    None
}
