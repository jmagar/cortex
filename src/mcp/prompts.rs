mod catalog;

pub(super) use catalog::{get_prompt, prompt_definitions};

#[cfg(test)]
use catalog::PROMPTS;

#[cfg(test)]
#[path = "prompts_tests.rs"]
mod tests;
