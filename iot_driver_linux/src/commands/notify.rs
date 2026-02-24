//! CLI command handlers for the notification system.

use super::CommandResult;

/// Run the notification daemon.
#[cfg(feature = "notify")]
pub async fn daemon(
    printer_config: Option<monsgeek_transport::PrinterConfig>,
    fps: u32,
    power_budget: u32,
) -> CommandResult {
    let kb = super::led_stream::open_with_patch_check(printer_config)?;

    let patch = kb.get_patch_info()?.unwrap();
    println!(
        "Patch: {} v{} (caps=0x{:04X})",
        patch.name, patch.version, patch.capabilities
    );

    iot_driver::notify::daemon::run(kb, fps, power_budget).await?;
    Ok(())
}

/// Helper to create a D-Bus proxy for the notify daemon.
#[cfg(feature = "notify")]
async fn notify_proxy() -> Result<zbus::Proxy<'static>, Box<dyn std::error::Error>> {
    let conn = zbus::Connection::session().await?;
    let proxy = zbus::Proxy::new_owned(
        conn,
        "org.monsgeek.Notify1",
        "/org/monsgeek/Notify1",
        "org.monsgeek.Notify1",
    )
    .await?;
    Ok(proxy)
}

/// Post a notification via D-Bus.
#[cfg(feature = "notify")]
pub async fn notify(
    key: &str,
    effect: &str,
    var_args: &[String],
    priority: i32,
    ttl_ms: i32,
    source: &str,
) -> CommandResult {
    let vars = super::effect::parse_vars(var_args)?;
    let proxy = notify_proxy().await?;

    let reply = proxy
        .call_method("Notify", &(source, key, effect, priority, ttl_ms, vars))
        .await?;
    let id: u64 = reply.body().deserialize()?;

    println!("Notification posted: id={id}");
    Ok(())
}

/// Acknowledge notifications via D-Bus.
#[cfg(feature = "notify")]
pub async fn ack(
    id: Option<u64>,
    key: Option<&str>,
    source: Option<&str>,
    all: bool,
) -> CommandResult {
    let proxy = notify_proxy().await?;

    if all {
        proxy.call_method("Clear", &()).await?;
        println!("All notifications cleared.");
    } else if let Some(id) = id {
        proxy.call_method("Acknowledge", &(id,)).await?;
        println!("Acknowledged notification {id}.");
    } else if let Some(key) = key {
        proxy.call_method("AcknowledgeKey", &(key,)).await?;
        println!("Acknowledged notifications on key '{key}'.");
    } else if let Some(source) = source {
        proxy.call_method("AcknowledgeSource", &(source,)).await?;
        println!("Acknowledged notifications from source '{source}'.");
    } else {
        eprintln!("Specify --id, --key, --source, or --all");
    }

    Ok(())
}

/// List active notifications via D-Bus.
#[cfg(feature = "notify")]
pub async fn list() -> CommandResult {
    let proxy = notify_proxy().await?;

    let reply = proxy.call_method("List", &()).await?;
    let items: Vec<(u64, String, String, String, i32)> = reply.body().deserialize()?;

    if items.is_empty() {
        println!("No active notifications.");
    } else {
        println!(
            "{:<6} {:<10} {:<10} {:<10} {:<8}",
            "ID", "Key", "Source", "Effect", "Priority"
        );
        println!("{}", "-".repeat(50));
        for (id, key, source, effect, priority) in &items {
            println!(
                "{:<6} {:<10} {:<10} {:<10} {:<8}",
                id, key, source, effect, priority
            );
        }
    }

    Ok(())
}

/// Clear all notifications via D-Bus.
#[cfg(feature = "notify")]
pub async fn clear() -> CommandResult {
    let proxy = notify_proxy().await?;
    proxy.call_method("Clear", &()).await?;
    println!("All notifications cleared.");
    Ok(())
}
