//! Real Secret Service exercise; run only inside the isolated test keyring.
use yigou_core::security::{SystemVault, Vault};
fn main() -> anyhow::Result<()> {
    let id = format!("test-only-{}", uuid::Uuid::new_v4());
    let value = zeroize::Zeroizing::new(uuid::Uuid::new_v4().to_string());
    let vault = SystemVault;
    vault.set(&id, &value)?;
    let fetched = zeroize::Zeroizing::new(
        vault
            .get(&id)?
            .ok_or_else(|| anyhow::anyhow!("missing test entry"))?,
    );
    if *fetched != *value {
        anyhow::bail!("keyring round-trip mismatch");
    }
    vault.delete(&id)?;
    if vault.get(&id)?.is_some() {
        anyhow::bail!("test entry not removed");
    }
    println!("PASS: SystemVault set/get/delete through isolated Secret Service");
    Ok(())
}
