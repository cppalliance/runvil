//! Reading what a prompt declares before it runs.

use runvil_engine::ErrorCode;
use runvil_engine::Prompt;
use runvil_engine::model::{ModelRole, ThinkingMode};

use super::prepare::refusal;
use super::support::{Built, Scenario, TestResult, model, prompt};

const CONTRACT: &str = "
    input: { path: names.md, description: The names to greet }
    output: { path: note.md, description: The note the greeter leaves }
    uses: [text]
    args: { times: { type: integer, optional: true, default: 1 }, name: { type: string } }
    models: { fast: {} }
";

#[test]
fn the_frontmatter_lists_every_declaration_with_arguments_sorted_by_name() -> TestResult {
    let source = prompt(CONTRACT, &[("Greet", "return 'done'")]);
    let parsed = Prompt::parse(&|_: &str| Ok(source.clone()), "prompt").0?;
    let frontmatter = parsed.frontmatter();
    assert_eq!(frontmatter.name(), "greeter");
    assert_eq!(
        frontmatter.description(),
        "A prompt the facade suite builds."
    );

    let files: Vec<_> = [frontmatter.input(), frontmatter.output()]
        .into_iter()
        .flatten()
        .map(|file| (file.path().to_string(), file.description().to_owned()))
        .collect();
    assert_eq!(
        files,
        [
            ("names.md".to_owned(), "The names to greet".to_owned()),
            (
                "note.md".to_owned(),
                "The note the greeter leaves".to_owned()
            ),
        ]
    );
    let packs: Vec<_> = frontmatter
        .uses()
        .iter()
        .map(|entry| (entry.vendor(), entry.id().to_string()))
        .collect();
    assert_eq!(packs, [("runvil", "text".to_owned())]);
    let roles: Vec<_> = ModelRole::ALL
        .into_iter()
        .filter_map(|role| {
            frontmatter
                .role_requirements(role)
                .map(|declared| (role, declared.min_context()))
        })
        .collect();
    assert_eq!(roles, [(ModelRole::Fast, None)]);

    assert!(
        frontmatter
            .args()
            .iter()
            .map(|(name, _)| name)
            .eq(["name", "times"])
    );

    let misspelled = source.replace("uses:", "use:");
    let parsed = Prompt::parse(&|_: &str| Ok(misspelled.clone()), "prompt").0;
    assert_eq!(
        parsed.err().map(|error| error.code),
        Some(ErrorCode::FrontmatterInvalid)
    );
    Ok(())
}

#[test]
fn the_prompt_is_named_by_the_name_its_finder_was_asked_for() -> TestResult {
    let source = prompt(CONTRACT, &[("Greet", "return 'done'")]);
    let parsed = Prompt::parse(&|_: &str| Ok(source.clone()), "greeter.md").0?;
    assert_eq!(parsed.file(), "greeter.md");
    Ok(())
}

#[test]
fn a_role_check_by_hand_finds_the_same_shortfall_the_run_reports() -> TestResult {
    let source = prompt(
        "models: { fast: {}, small: { thinking: false, min_context: 100000 } }",
        &[("Greet", "return 'done'")],
    );
    let model = model("canned", 200_000, ThinkingMode::Always)?;
    let scenario = Scenario::default().model(model.clone());
    let Built { prompt, .. } = scenario.build(&source)?;

    let mut misses = Vec::new();
    for role in ModelRole::ALL {
        let Some(declared) = prompt.frontmatter().role_requirements(role) else {
            continue;
        };
        let thinking_miss = match declared.thinking() {
            Some(true) => model.thinking() == ThinkingMode::Never,
            Some(false) => model.thinking() == ThinkingMode::Always,
            None => false,
        };
        let context_miss = declared
            .min_context()
            .is_some_and(|min| min > model.context());
        if thinking_miss || context_miss {
            misses.push((role, thinking_miss, context_miss));
        }
    }
    assert_eq!(misses, [(ModelRole::Small, true, false)]);

    assert_eq!(
        refusal(scenario, &source)?,
        "the environment cannot satisfy this prompt:\n\
         - role 'small': requires 'thinking: false'; \
         the thinking capability of the model filling it is Always (at prompt:5)",
        "only the small role falls short, on its thinking requirement"
    );
    Ok(())
}
