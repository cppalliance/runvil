//! Calls a typed Rust tool from Lua and reads the prompt's declared output.

use runvil::plugin::{PackInfo, PackRegistry};
use runvil::{Error, Host, OutputTrust};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LineItem {
    /// Price of one item, in cents.
    unit_price_cents: u32,
    /// Number of items to purchase.
    quantity: u16,
}

#[derive(Serialize)]
struct LineTotal {
    total_cents: u64,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let mut packs = PackRegistry::new();
    packs.define(
        PackInfo::new("examples/pricing").description("Calculate line item totals"),
        |pack| {
            pack.tool(
                "total",
                "Multiply a unit price in cents by a quantity",
                OutputTrust::Trusted,
                |item: LineItem| async move {
                    Ok(LineTotal {
                        total_cents: u64::from(item.unit_price_cents) * u64::from(item.quantity),
                    })
                },
            )
        },
    )?;
    let prompts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/prompts");
    let host = Host::new(prompts).with_packs(packs);
    let receipt = host.run("receipt.md").output().await?;
    assert_eq!(receipt, "3 items: 3750 cents");
    println!("{receipt}");
    Ok(())
}
