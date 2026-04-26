use crate::{
    cmd,
    config::{Config, PrfItem, PrfOption, profiles::profiles_draft_update_item_safe},
    core::{CoreManager, handle, tray},
    utils::help::{mask_err, mask_url},
};
use anyhow::{Result, bail};
use clash_verge_logging::{Type, logging, logging_error};
use smartstring::alias::String;
use tauri::Emitter as _;
use tauri_plugin_mihomo::models::Proxies;

/// Toggle proxy profile
pub async fn toggle_proxy_profile(profile_index: String) {
    logging_error!(
        Type::Config,
        cmd::patch_profiles_config_by_profile_index(profile_index).await
    );
}

pub async fn switch_proxy_node(group_name: &str, proxy_name: &str) {
    match handle::Handle::mihomo()
        .await
        .select_node_for_group(group_name, proxy_name)
        .await
    {
        Ok(_) => {
            logging!(info, Type::Tray, "切换代理成功: {} -> {}", group_name, proxy_name);
            let _ = handle::Handle::app_handle().emit("verge://refresh-proxy-config", ());
            let _ = tray::Tray::global().update_menu().await;
            return;
        }
        Err(err) => {
            logging!(
                error,
                Type::Tray,
                "切换代理失败: {} -> {}, 错误: {:?}",
                group_name,
                proxy_name,
                err
            );
        }
    }

    match handle::Handle::mihomo()
        .await
        .select_node_for_group(group_name, proxy_name)
        .await
    {
        Ok(_) => {
            logging!(info, Type::Tray, "代理切换回退成功: {} -> {}", group_name, proxy_name);
            let _ = tray::Tray::global().update_menu().await;
        }
        Err(err) => {
            logging!(
                error,
                Type::Tray,
                "代理切换最终失败: {} -> {}, 错误: {:?}",
                group_name,
                proxy_name,
                err
            );
        }
    }
}

async fn should_update_profile(uid: &String, ignore_auto_update: bool) -> Result<Option<(String, Option<PrfOption>)>> {
    let profiles = Config::profiles().await;
    let profiles = profiles.latest_arc();
    let item = profiles.get_item(uid)?;
    let is_remote = item.itype.as_ref().is_some_and(|s| s == "remote");

    if !is_remote {
        logging!(info, Type::Config, "[订阅更新] {uid} 不是远程订阅，跳过更新");
        Ok(None)
    } else if item.url.is_none() {
        logging!(warn, Type::Config, "Warning: [订阅更新] {uid} 缺少URL，无法更新");
        bail!("failed to get the profile item url");
    } else if !ignore_auto_update && !item.option.as_ref().and_then(|o| o.allow_auto_update).unwrap_or(true) {
        logging!(info, Type::Config, "[订阅更新] {} 禁止自动更新，跳过更新", uid);
        Ok(None)
    } else {
        logging!(
            info,
            Type::Config,
            "[订阅更新] {} 是远程订阅，URL: {}",
            uid,
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

async fn perform_profile_update(
    uid: &String,
    url: &String,
    opt: Option<&PrfOption>,
    option: Option<&PrfOption>,
    is_mannual_trigger: bool,
) -> Result<bool> {
    logging!(info, Type::Config, "[订阅更新] 开始下载新的订阅内容");
    let mut merged_opt = PrfOption::merge(opt, option);
    let is_current = {
        let profiles = Config::profiles().await;
        profiles.latest_arc().is_current_profile_index(uid)
    };
    let profiles = Config::profiles().await;
    let profiles_arc = profiles.latest_arc();
    let profile_name = profiles_arc
        .get_name_by_uid(uid)
        .cloned()
        .unwrap_or_else(|| String::from("UnKnown Profile"));

    let mut last_err;

    match PrfItem::from_url(url, None, None, merged_opt.as_ref()).await {
        Ok(mut item) => {
            logging!(info, Type::Config, "[订阅更新] 更新订阅配置成功");
            profiles_draft_update_item_safe(uid, &mut item).await?;
            return Ok(is_current);
        }
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [订阅更新] 正常更新失败: {}，尝试使用Clash代理更新",
                mask_err(&err.to_string())
            );
            last_err = err;
        }
    }

    merged_opt.get_or_insert_with(PrfOption::default).self_proxy = Some(true);
    merged_opt.get_or_insert_with(PrfOption::default).with_proxy = Some(false);

    match PrfItem::from_url(url, None, None, merged_opt.as_ref()).await {
        Ok(mut item) => {
            logging!(info, Type::Config, "[订阅更新] 使用 Clash代理 更新订阅配置成功");
            profiles_draft_update_item_safe(uid, &mut item).await?;
            handle::Handle::notice_message("update_with_clash_proxy", profile_name);
            drop(last_err);
            return Ok(is_current);
        }
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [订阅更新] Clash代理更新失败: {}，尝试使用系统代理更新",
                mask_err(&err.to_string())
            );
            last_err = err;
        }
    }

    merged_opt.get_or_insert_with(PrfOption::default).self_proxy = Some(false);
    merged_opt.get_or_insert_with(PrfOption::default).with_proxy = Some(true);

    match PrfItem::from_url(url, None, None, merged_opt.as_ref()).await {
        Ok(mut item) => {
            logging!(info, Type::Config, "[订阅更新] 使用 系统代理 更新订阅配置成功");
            profiles_draft_update_item_safe(uid, &mut item).await?;
            handle::Handle::notice_message("update_with_clash_proxy", profile_name);
            drop(last_err);
            return Ok(is_current);
        }
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [订阅更新] 系统代理更新失败: {}，所有重试均已失败",
                mask_err(&err.to_string())
            );
            last_err = err;
        }
    }

    if is_mannual_trigger {
        handle::Handle::notice_message("update_failed_even_with_clash", format!("{profile_name} - {last_err}"));
    }
    Ok(is_current)
}

pub async fn update_profile(
    uid: &String,
    option: Option<&PrfOption>,
    auto_refresh: bool,
    ignore_auto_update: bool,
    is_mannual_trigger: bool,
) -> Result<()> {
    logging!(info, Type::Config, "[订阅更新] 开始更新订阅 {}", uid);
    let url_opt = should_update_profile(uid, ignore_auto_update).await?;

    let should_refresh = match url_opt {
        Some((url, opt)) => {
            perform_profile_update(uid, &url, opt.as_ref(), option, is_mannual_trigger).await? && auto_refresh
        }
        None => auto_refresh,
    };

    if should_refresh {
        logging!(info, Type::Config, "[订阅更新] 更新内核配置");
        match CoreManager::global().update_config().await {
            Ok(_) => {
                logging!(info, Type::Config, "[订阅更新] 更新成功");
                handle::Handle::refresh_clash();
            }
            Err(err) => {
                logging!(error, Type::Config, "[订阅更新] 更新失败: {}", err);
                handle::Handle::notice_message("update_failed", format!("{err}"));
                logging!(error, Type::Config, "{err}");
            }
        }
    }

    Ok(())
}

/// 增强配置
pub async fn enhance_profiles() -> Result<(bool, String)> {
    crate::core::CoreManager::global().update_config().await
}

/// 刷新所有远程订阅的服务器列表
/// 这与update_profile不同，它只刷新订阅的服务器列表，不触发完整的配置更新
pub async fn refresh_all_remote_subscriptions() -> Result<()> {
    use crate::config::profiles::profiles_draft_update_item_safe;

    logging!(info, Type::Config, "[订阅刷新] 开始刷新所有远程订阅");

    let profiles = Config::profiles().await;
    let items = match profiles.latest_arc().get_items() {
        Some(items) => items.clone(),
        None => {
            logging!(warn, Type::Config, "[订阅刷新] 无法获取订阅列表");
            return Ok(());
        }
    };

    let mut refreshed_count = 0;
    let mut failed_count = 0;

    for item in items.iter() {
        // 只处理远程订阅类型的profile
        let is_remote = item.itype.as_ref().is_some_and(|s| s == "remote");
        if !is_remote {
            continue;
        }

        let Some(uid) = item.uid.as_ref() else {
            continue;
        };

        let Some(url) = item.url.as_ref() else {
            continue;
        };

        logging!(info, Type::Config, "[订阅刷新] 刷新订阅: {}", uid);

        // 使用订阅的URL重新获取内容，但保留原有的UID和配置选项
        match PrfItem::from_url(url, None, None, item.option.as_ref()).await {
            Ok(mut new_item) => {
                // 保留原有的UID
                new_item.uid = item.uid.clone();
                new_item.option = item.option.clone();

                if let Err(e) = profiles_draft_update_item_safe(uid, &mut new_item).await {
                    logging!(error, Type::Config, "[订阅刷新] 刷新订阅失败 {}: {}", uid, e);
                    failed_count += 1;
                } else {
                    refreshed_count += 1;
                    logging!(info, Type::Config, "[订阅刷新] 订阅刷新成功: {}", uid);
                    // 通知前端更新界面
                    handle::Handle::notify_profile_update_completed(uid);
                }
            }
            Err(e) => {
                logging!(error, Type::Config, "[订阅刷新] 获取订阅数据失败 {}: {}", uid, e);
                failed_count += 1;
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

    Ok(())
}

/// 自动选择最佳代理节点
/// 遍历所有代理组，选择延迟最低的节点
pub async fn auto_select_best_proxies() -> Result<()> {
    use tauri_plugin_mihomo::models::Proxies;

    logging!(info, Type::Config, "[自动选优] 开始自动选择最佳代理节点");

    let proxies_data: Proxies = match handle::Handle::mihomo().await.get_proxies().await {
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
        let best_proxy = find_best_proxy_from_history(&proxies_data, all_proxies, &now_proxy);

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

    logging!(
        info,
        Type::Config,
        "[自动选优] 完成: 切换了 {} 个组",
        switched_count
    );

    Ok(())
}

/// 从延迟历史记录中找到最佳代理
fn find_best_proxy_from_history<'a>(
    proxies_data: &'a Proxies,
    all_proxies: &'a [std::string::String],
    current_proxy: &str,
) -> Option<std::string::String> {
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
