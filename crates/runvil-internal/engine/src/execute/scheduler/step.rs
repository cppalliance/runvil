//! One chain's step to its next suspension point: resume a suspended
//! coroutine with its delivered answer, or advance the walk, then apply the
//! block coroutine's outcome: park a yielded chain on its request's
//! dispatch, advance or finish a completed block, and report the chunk's
//! closing observation boundary.

use runvil_types::emitter::Emitter;
use runvil_types::event::lifecycle;

use crate::heading_address::{SectionIndex, SectionPos};
use crate::lua::{Answer, CoroStep, LuaBlockResult, Request, YieldParse};
use crate::parser::{Block, BodyText};
use crate::{Error, Result};

use super::chain::ChainStep;
use super::chain_record::{Entered, Running};
use super::{ChainIndex, ChainOps, Scheduler};

/// The notice a failed H1 gate ends the run with; the chunk's own failure
/// follows it as the cause.
const H1_GATE_FAILED: &str = "the environment cannot satisfy this prompt:\n- the H1 gate failed";

/// What a Lua block's coroutine step leaves the chain to do, settled under
/// the borrow of its entered section so the action can touch the
/// scheduler's other fields.
#[derive(Debug)]
enum Settled {
    /// The block suspended on a request for its arm to dispatch.
    Dispatch(Request),
    /// The block suspended on a call answered on the spot.
    Answer(Result<Answer<Error>>),
    /// The block jumped to the heading.
    Jump(String),
    /// The block returned a scalar.
    Returned(String),
    /// The block returned no value; the section's next block is next.
    Advanced,
}

/// Settles one Lua block coroutine step in `entered`, the block's
/// section, and reports the chunk's closing observation boundary under
/// `name`: a yield parses into its request and leaves the block
/// suspended, and a completion moves past the block.
fn settle_block(
    entered: &mut Entered,
    result: Result<CoroStep>,
    is_h1: bool,
    emitter: &Emitter,
    name: &str,
) -> Result<Settled> {
    let step = match result {
        Ok(step) => step,
        Err(error) => {
            emitter.report(name, lifecycle::LUA_CHUNK_FAILED);
            // A failed H1 assertion ends the run before the walk:
            // H1's remaining job is the prompt's hard gates, so the
            // prompt chunk's own Lua failure IS the failed assertion,
            // kept whole as the refusal's cause. Only the chunk's
            // error remaps: the machinery around it (section setup,
            // the final `var` read-back, jump-target resolution)
            // keeps its own kind - a prompt bug under
            // `Error::Lua`, not an unsatisfiable environment. Fatal
            // run conditions (cancellation, the claims violation)
            // keep their own classification either way.
            return Err(match error {
                error @ Error::Lua { .. } if is_h1 => Error::RequirementsUnmet {
                    notice: H1_GATE_FAILED.to_owned(),
                    cause: Some(Box::new(error)),
                },
                other => other,
            });
        }
    };
    match step {
        CoroStep::Yielded(coroutine, values) => {
            let settled = match entered.frame.vm().request_from_yield(&values) {
                YieldParse::Request(request) => Settled::Dispatch(request),
                YieldParse::Refused(error) => Settled::Answer(Err(Error::from(error))),
                YieldParse::Malformed(error) => {
                    emitter.report(name, lifecycle::LUA_CHUNK_FAILED);
                    return Err(Error::from(error));
                }
            };
            entered.running = Some(Running {
                coroutine,
                answer: None,
            });
            Ok(settled)
        }
        CoroStep::Done(done) => {
            // A jump is a control transfer, not a failure: the chunk
            // boundary reports success and the walk moves to the
            // resolved target.
            emitter.report(name, lifecycle::LUA_CHUNK_SUCCEEDED);
            match done {
                LuaBlockResult::Jump(heading) => Ok(Settled::Jump(heading)),
                LuaBlockResult::Returned(Some(value)) => {
                    // The H1 pass's final `var` read-back runs here
                    // exactly as the walk reads it on every exit, so a
                    // reassigned `var` global fails the run instead of
                    // returning the value.
                    if is_h1 {
                        entered.frame.read_var()?;
                    }
                    Ok(Settled::Returned(value))
                }
                LuaBlockResult::Returned(None) => {
                    entered.block = entered.block.next();
                    Ok(Settled::Advanced)
                }
            }
        }
    }
}

impl Scheduler {
    /// Runs one ready chain to its next suspension point: resume a
    /// suspended coroutine with its delivered answer, or advance the walk.
    /// The step never awaits: every dispatch either issues its leaf effect
    /// or answers on the spot.
    ///
    /// The driver owns the chunk observation boundaries: STARTED at a
    /// block's start, SUCCEEDED or FAILED when its coroutine finally
    /// returns or fails - a suspension is neither.
    pub(super) fn step_chain(&mut self, id: ChainIndex) -> Result<ChainStep> {
        let prompt = self.prompt();
        let chain = &mut self.chains[id];
        let blocks = chain.blocks(&prompt);
        let is_h1 = chain.kind.is_h1();
        let name = chain.section_name().to_owned();
        let emitter = chain.ctx.emitter();
        let Some(entered) = chain.entered.as_mut() else {
            // Between sections: enter the next section, or end the chain
            // when the slice is exhausted.
            return self.advance_entry(id);
        };
        let result = if let Some(running) = entered.running.take() {
            let Running {
                coroutine,
                answer: Some(answer),
            } = running
            else {
                return Err(Error::internal(
                    "a ready chain's suspended coroutine waits on its answer",
                ));
            };
            // The answer ends whatever the chain was parked on.
            chain.parked = None;
            let Some(Block::Lua(program)) = entered.block_in(blocks) else {
                return Err(Error::internal("a suspended coroutine's block is Lua"));
            };
            entered
                .frame
                .vm()
                .resume_block_coro_answer(program, &coroutine, answer)
        } else {
            match entered.block_in(blocks) {
                Some(Block::Lua(program)) => {
                    // The pending Markdown buffer installs as the block's
                    // fresh read-only lazy `prose` template first.
                    emitter.report(&name, lifecycle::LUA_CHUNK_STARTED);
                    let pending = entered.pending_prose.take();
                    let template = pending.as_ref().map_or("", BodyText::as_str);
                    if let Err(error) = entered.frame.install_lazy_prose(&chain.ctx, template) {
                        emitter.report(&name, lifecycle::LUA_CHUNK_FAILED);
                        return Err(error);
                    }
                    entered
                        .frame
                        .vm()
                        .start_block_coro(program)
                        .map_err(Error::from)
                }
                Some(Block::Prose { text, .. }) => {
                    // The parser emits one prose block per inter-fence gap,
                    // already accumulated and reset at thematic breaks, so the
                    // block IS the pending buffer the next Lua fence consumes.
                    // Prose never infers: the buffer waits for the following
                    // block's lazy `prose` install, unevaluated until read.
                    entered.pending_prose = Some(text.clone());
                    entered.block = entered.block.next();
                    self.ready.push_back(id);
                    return Ok(ChainStep::Continue);
                }
                // `Block` is `#[non_exhaustive]` across the crate seam; an
                // unlisted variant has no advance rule.
                Some(_) => {
                    return Err(Error::internal("an unrecognized block kind cannot advance"));
                }
                // The section's blocks are exhausted: fall through.
                None if is_h1 => {
                    return self.end_h1(id, SectionPos::top(SectionIndex::FIRST));
                }
                None => {
                    self.with_chain(id, ChainOps::end_section)?;
                    self.ready.push_back(id);
                    return Ok(ChainStep::Continue);
                }
            }
        };
        let settled = settle_block(entered, result, is_h1, emitter, &name)?;
        self.apply_settled(id, settled, is_h1)
    }

    /// Acts on a settled Lua block for chain `id`: dispatches a suspended
    /// block's request or answers it on the spot, moves the walk for a
    /// jump, ends the chain or the run for a scalar return, and requeues
    /// the chain for its section's next block.
    fn apply_settled(
        &mut self,
        id: ChainIndex,
        settled: Settled,
        is_h1: bool,
    ) -> Result<ChainStep> {
        match settled {
            Settled::Dispatch(request) => self.dispatch(id, request)?,
            Settled::Answer(answer) => self.answer_inline(id, answer)?,
            // A jump out of H1 ends the pass and starts the walk at the
            // target.
            Settled::Jump(heading) if is_h1 => return self.end_h1_at_jump(id, &heading),
            Settled::Jump(heading) => {
                self.with_chain(id, |ops| ops.apply_jump(&heading))?;
                self.ready.push_back(id);
            }
            Settled::Returned(value) if is_h1 => {
                // A scalar return from the H1 pass short-circuits the
                // whole run: the frame drops unarmed - the pass never
                // fires SECTION_FINISHED - and the run ends here, so the
                // pass's live tasks end under the chain-end rules as at
                // any chain end.
                self.chains[id].entered = None;
                return Ok(ChainStep::RunEnded(self.settle_owned_tasks(id, Ok(value))));
            }
            // A scalar return ends the chain it fired in.
            Settled::Returned(value) => return Ok(self.finish(id, Ok(Some(value)))),
            Settled::Advanced => self.ready.push_back(id),
        }
        Ok(ChainStep::Continue)
    }
}
