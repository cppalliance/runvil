//! Handing Packs to a run.

use runvil::packs::{PackId, RosterEntry};

use super::prepare::refusal;
use super::support::{Scenario, TestResult, greeter, prompt};

#[test]
fn two_runs_whose_configs_carry_different_rosters_each_see_their_own() -> TestResult {
    let source = greeter("return tostring(tools.packs.get('kit') ~= nil)");
    let kit = PackId::parse("kit")?;
    let usable = {
        let kit = kit.clone();
        Scenario::default().config(move |ctx| {
            ctx.with_packs([(
                kit.clone(),
                RosterEntry::Usable {
                    vendor: "acme".to_owned(),
                },
            )])
        })
    };
    let unavailable = Scenario::default().config(move |ctx| {
        ctx.with_packs([(
            kit.clone(),
            RosterEntry::Unavailable {
                reason: "down".to_owned(),
            },
        )])
    });

    for (scenario, expected) in [(usable, "true"), (unavailable, "false")] {
        assert_eq!(scenario.run(&source)?.text()?, expected);
    }
    Ok(())
}

#[test]
fn a_declared_pack_the_roster_lacks_or_cannot_use_refuses_the_run() -> TestResult {
    let source = prompt("uses: [kit, web]", &[("Only", "return 'done'")]);
    let kit = PackId::parse("kit")?;
    let scenario = Scenario::default().config(move |ctx| {
        ctx.with_packs([(
            kit.clone(),
            RosterEntry::Unavailable {
                reason: "the kit server is down".to_owned(),
            },
        )])
    });
    assert_eq!(
        refusal(scenario, &source)?,
        "the environment cannot satisfy this prompt:\n\
         - missing required Pack: web\n\
         - kit is unavailable: the kit server is down"
    );
    Ok(())
}
