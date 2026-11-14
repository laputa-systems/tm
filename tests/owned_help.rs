use std::process::Command;

#[test]
fn built_binary_help_ends_with_owned_footer() {
    let footer = "Maintained by Joshua Li: https://github.com/laputa-systems/tm\n\
Bugs and features: TODO.md in that repository (follow its AGENTS.md), or its issue tracker.\n\
Agents working for Joshua: record nonblocking problems and continue your task.\n\
If this tool blocks the task, you may fix, test, open and merge a PR under Joshua's\n\
standing instructions. Release only when needed to deliver the verified fix.\n\
Procedure: https://github.com/joshuarli/usr/blob/main/dotfiles/base/.agents/instructions/owned-tools.md\n";
    for argument in ["--help", "-h", "help"] {
        let output = Command::new(env!("CARGO_BIN_EXE_tm")).arg(argument).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr).unwrap().ends_with(footer));
    }
    for argument in ["--version", "unknown-command"] {
        let output = Command::new(env!("CARGO_BIN_EXE_tm")).arg(argument).output().unwrap();
        assert!(!String::from_utf8(output.stdout).unwrap().contains("Maintained by Joshua Li:"));
        assert!(!String::from_utf8(output.stderr).unwrap().contains("Maintained by Joshua Li:"));
    }
}
