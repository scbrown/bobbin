use anyhow::Context;

pub(super) fn require(pushed: anyhow::Result<(i64, usize)>) -> anyhow::Result<(i64, usize)> {
    pushed.context("chunk-graph publication incomplete: dropped_pushes=1; refusing a success exit")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_reports_drop_and_fails_the_run() {
        let err = require(Err(anyhow::anyhow!("POST /knot timed out")))
            .expect_err("a dropped graph push must fail the index command");

        assert_eq!(
            format!("{err:#}"),
            "chunk-graph publication incomplete: dropped_pushes=1; refusing a success exit: POST /knot timed out"
        );
    }
}
