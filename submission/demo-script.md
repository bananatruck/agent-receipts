# Demo script

Target length is 2 minutes 40 seconds, below the three-minute limit.
All reports shown are synthetic and labelled as such.
Use the real output on screen and keep the source report visible beside it.
The report heading stays visible as not checkable; only blank lines and recognized fence delimiters are omitted.

| Time | Screen and narration |
| --- | --- |
| 0:00 to 0:20 | Show the synthetic history report from the README. Explain that each line states something the reviewer can check. |
| 0:20 to 0:45 | Run the README history command against this repository with the specified base and a freshly captured test log. Point to the confirmed commit and the contradicted missing file. |
| 0:45 to 1:10 | Point to the real passed count in the captured log. Show the matching and mismatching report counts and read both verdicts. Explain that this checks supplied text, not authentic execution. |
| 1:10 to 1:35 | Show the addition and exclusion checks beside the base-to-HEAD diff. Explain that the scope is committed changes and that rename checks inspect both endpoints. |
| 1:35 to 1:55 | Run the same report with `--json`. Show `schema_version`, one statement's evidence, the summary, and the exit code. |
| 1:55 to 2:15 | Run `sh examples/demo.sh`. Show the clean-tree check changing after an untracked file appears. |
| 2:15 to 2:35 | Show `Not pushed` staying not checkable without a named local tracking ref. Explain that a bare claim checks HEAD against the supplied local ref and that this ref may be stale. |
| 2:35 to 2:40 | End on the source and documented limits. State that missing evidence stays not checkable. |

Prepare the release binary and capture the test log before recording so compilation time is outside the video.
The history report contains intentional contradictions, so exit 1 is expected.
If test counts change, update the synthetic report to match the captured run and keep one intentionally mismatched count.
No recording or upload has been completed by this script.
