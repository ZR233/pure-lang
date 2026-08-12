use pl_model::EffectivePromptCachePolicy;
use pl_protocol::Result;

use crate::derive_prompt_cache_key;
use crate::session::AgentSession;
use crate::turn::TurnOptions;

pub(super) fn sync(
    session: &mut AgentSession,
    options: &TurnOptions,
    policy: EffectivePromptCachePolicy,
) -> Result<()> {
    if options.prompt_cache_key.is_some() {
        return Ok(());
    }
    let key = match (
        policy.uses_prompt_cache_key(),
        options.prompt_cache_namespace.as_deref(),
    ) {
        (true, Some(namespace)) => current(session, &options.prompt_scope)
            .map(|prompt| derive_prompt_cache_key(namespace, prompt))
            .transpose()?,
        _ => None,
    };
    session.replace_prompt_cache_key(key);
    Ok(())
}

pub(super) fn current<'a>(
    session: &'a AgentSession,
    scope: &str,
) -> Option<&'a pl_protocol::ThreadPromptSnapshot> {
    session.prompt_metadata().slots.get(scope)
}
