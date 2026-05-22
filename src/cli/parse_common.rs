use super::*;
pub(crate) struct FlagCursor<'a> {
    args: &'a [String],
    index: usize,
}

impl<'a> FlagCursor<'a> {
    pub(crate) fn new(args: &'a [String]) -> Self {
        Self { args, index: 0 }
    }

    pub(crate) fn next(&mut self) -> Option<String> {
        let value = self.args.get(self.index)?.clone();
        self.index += 1;
        Some(value)
    }

    pub(crate) fn value(&mut self, flag: &str) -> Result<String> {
        let value = self
            .args
            .get(self.index)
            .ok_or_else(|| anyhow!("{flag} requires a value"))?
            .clone();
        if value.starts_with('-') {
            bail!("{flag} requires a value");
        }
        self.index += 1;
        Ok(value)
    }

    pub(crate) fn match_value(&mut self, arg: &str, flag: &str) -> Result<Option<String>> {
        if arg == flag {
            return Ok(Some(self.value(flag)?));
        }
        if let Some(rest) = arg.strip_prefix(flag).and_then(|s| s.strip_prefix('=')) {
            if rest.is_empty() {
                bail!("{flag} requires a value");
            }
            return Ok(Some(rest.to_string()));
        }
        Ok(None)
    }
}

pub(crate) fn value_after_equals(arg: String, flag: &str) -> Result<String> {
    let prefix = format!("{flag}=");
    let value = arg
        .strip_prefix(&prefix)
        .ok_or_else(|| anyhow!("expected {flag}=<value>"))?;
    if value.is_empty() {
        bail!("{flag} requires a value");
    }
    Ok(value.to_string())
}

pub(crate) fn parse_u32_flag(flag: &str, value: String) -> Result<u32> {
    value
        .parse::<u32>()
        .map_err(|_| anyhow!("{flag} must be an unsigned integer"))
}

pub(crate) fn parse_i64_flag(flag: &str, value: String) -> Result<i64> {
    value
        .parse::<i64>()
        .map_err(|e| anyhow!("{flag} must be a number: {e}"))
}

pub(crate) fn parse_positive_u64_flag(flag: &str, value: String) -> Result<u64> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| anyhow!("{flag} expects a positive integer"))?;
    if parsed == 0 {
        bail!("{flag} expects a positive integer");
    }
    Ok(parsed)
}

pub(crate) fn parse_output_args(command: &str, args: &[String]) -> Result<OutputArgs> {
    let mut parsed = OutputArgs::default();
    for arg in args {
        match arg.as_str() {
            "--json" => parsed.json = true,
            _ => bail!("unknown {command} option: {arg}"),
        }
    }
    Ok(parsed)
}

#[cfg(test)]
#[path = "parse_common_tests.rs"]
mod tests;
