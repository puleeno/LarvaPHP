//! Shell completion script generation.
//!
//! Produces completion scripts for the `larva-php` CLI without pulling in an
//! external dependency. Scripts are emitted as static templates with the
//! program name substituted.

use std::io::{self, Write};

/// Supported shells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
}

impl Shell {
    /// Parse a shell name (case-insensitive).
    pub fn from_name(name: &str) -> Option<Shell> {
        match name.to_ascii_lowercase().as_str() {
            "bash" => Some(Shell::Bash),
            "zsh" => Some(Shell::Zsh),
            "fish" => Some(Shell::Fish),
            "powershell" | "pwsh" | "power-shell" => Some(Shell::PowerShell),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::PowerShell => "powershell",
        }
    }

    /// Generate a completion script for the given binary name.
    pub fn generate(&self, bin: &str) -> String {
        match self {
            Shell::Bash => bash_script(bin),
            Shell::Zsh => zsh_script(bin),
            Shell::Fish => fish_script(bin),
            Shell::PowerShell => powershell_script(bin),
        }
    }

    /// Write the completion script to `out`.
    pub fn write_to(&self, bin: &str, out: &mut dyn Write) -> io::Result<()> {
        out.write_all(self.generate(bin).as_bytes())
    }
}

fn bash_script(bin: &str) -> String {
    format!(
        r#"# {bin} bash completion
_{bin}() {{
    local cur prev opts
    COMPREPLY=()
    cur="${{COMP_WORDS[COMP_CWORD]}}"
    prev="${{COMP_WORDS[COMP_CWORD-1]}}"
    opts="-r --root -a --addr --php-ini -v --version -h --help --completion"

    case "${{prev}}" in
        -r|--root)
            COMPREPLY=( $(compgen -d -- "${{cur}}") )
            return 0
            ;;
        -a|--addr|--completion)
            COMPREPLY=( $(compgen -W "127.0.0.1:8000 0.0.0.0:8000" -- "${{cur}}") )
            return 0
            ;;
        --php-ini)
            return 0
            ;;
    esac

    if [[ "${{cur}}" == -* ]]; then
        COMPREPLY=( $(compgen -W "${{opts}}" -- "${{cur}}") )
        return 0
    fi

    COMPREPLY=( $(compgen -f -- "${{cur}}") )
}}
complete -F _{bin} {bin}
"#,
        bin = bin
    )
}

fn zsh_script(bin: &str) -> String {
    format!(
        r#"#compdef {bin}
# {bin} zsh completion
_{bin}() {{
    local -a opts
    opts=(
        '(-r --root)'{{-r,--root}}'[Document root directory]:dir:_directories'
        '(-a --addr)'{{-a,--addr}}'[Bind address]:addr:'
        '--php-ini[Set a php.ini directive]:key=value:'
        '(-v --version)'{{-v,--version}}'[Show version]'
        '(-h --help)'{{-h,--help}}'[Show help]'
        '--completion[Generate shell completion]:shell:(bash zsh fish powershell)'
    )
    _arguments $opts
}}
compdef _{bin} {bin}
"#,
        bin = bin
    )
}

fn fish_script(bin: &str) -> String {
    format!(
        r#"# {bin} fish completion
complete -c {bin} -s r -l root -d "Document root directory" -r -F
complete -c {bin} -s a -l addr -d "Bind address" -r
complete -c {bin} -l php-ini -d "Set a php.ini directive" -r
complete -c {bin} -s v -l version -d "Show version"
complete -c {bin} -s h -l help -d "Show help"
complete -c {bin} -l completion -d "Generate shell completion" -x -a "bash zsh fish powershell"
"#,
        bin = bin
    )
}

fn powershell_script(bin: &str) -> String {
    format!(
        r#"# {bin} PowerShell completion
Register-ArgumentCompleter -Native -CommandName {bin} -ScriptBlock {{
    param($wordToComplete, $commandAst, $cursorPosition)
    $options = @(
        '--root', '--addr', '--php-ini', '--version', '--help', '--completion',
        '-r', '-a', '-v', '-h'
    )
    $options |
        Where-Object {{ $_ -like "$wordToComplete*" }} |
        ForEach-Object {{
            [System.Management.Automation.CompletionResult]::new(
                $_, $_, 'ParameterName', $_
            )
        }}
}}
"#,
        bin = bin
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_shell_names() {
        assert_eq!(Shell::from_name("bash"), Some(Shell::Bash));
        assert_eq!(Shell::from_name("ZSH"), Some(Shell::Zsh));
        assert_eq!(Shell::from_name("fish"), Some(Shell::Fish));
        assert_eq!(Shell::from_name("pwsh"), Some(Shell::PowerShell));
        assert_eq!(Shell::from_name("nonsense"), None);
    }

    #[test]
    fn bash_script_contains_flags() {
        let s = Shell::Bash.generate("larva-php");
        assert!(s.contains("--root"));
        assert!(s.contains("--php-ini"));
        assert!(s.contains("complete -F _larva-php larva-php"));
    }

    #[test]
    fn zsh_script_contains_compdef() {
        let s = Shell::Zsh.generate("larva-php");
        assert!(s.contains("#compdef larva-php"));
        assert!(s.contains("--root"));
    }

    #[test]
    fn fish_script_contains_complete() {
        let s = Shell::Fish.generate("larva-php");
        assert!(s.contains("complete -c larva-php"));
        assert!(s.contains("php-ini"));
    }

    #[test]
    fn powershell_script_contains_register() {
        let s = Shell::PowerShell.generate("larva-php");
        assert!(s.contains("Register-ArgumentCompleter"));
        assert!(s.contains("larva-php"));
    }

    #[test]
    fn shell_names_are_stable() {
        assert_eq!(Shell::Bash.name(), "bash");
        assert_eq!(Shell::Zsh.name(), "zsh");
        assert_eq!(Shell::Fish.name(), "fish");
        assert_eq!(Shell::PowerShell.name(), "powershell");
    }
}