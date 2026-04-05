//! Shell hook script generation (pure functions).

use super::shell_detect::ShellType;

/// Generate the shell hook script for the given shell type.
///
/// Pure function: takes shell type, returns the hook script as a string.
pub fn generate_hook(shell: ShellType) -> String {
    match shell {
        ShellType::Fish => generate_fish_hook(),
        ShellType::Bash => generate_bash_hook(),
        ShellType::Zsh => generate_zsh_hook(),
    }
}

fn generate_fish_hook() -> String {
    r#"function __clipboard2path_paste
    set -l latest_path "$XDG_RUNTIME_DIR/clipboard2path/latest-path"
    if test -f "$latest_path"
        set -l path (string trim -- (cat "$latest_path"))
        if test -n "$path"
            commandline -i -- $path
            return
        end
    end
    commandline -i -- (wl-paste -n 2>/dev/null)
end

bind \ev '__clipboard2path_paste'
bind -M insert \ev '__clipboard2path_paste'
"#
    .to_string()
}

fn generate_bash_hook() -> String {
    r#"clipboard2path_paste() {
    local latest_path="$XDG_RUNTIME_DIR/clipboard2path/latest-path"
    if [[ -f "$latest_path" ]]; then
        local path
        path="$(cat "$latest_path")"
        if [[ -n "$path" ]]; then
            READLINE_LINE="${READLINE_LINE:0:$READLINE_POINT}${path}${READLINE_LINE:$READLINE_POINT}"
            READLINE_POINT=$(( READLINE_POINT + ${#path} ))
            return
        fi
    fi
    local text
    text="$(wl-paste -n 2>/dev/null)"
    READLINE_LINE="${READLINE_LINE:0:$READLINE_POINT}${text}${READLINE_LINE:$READLINE_POINT}"
    READLINE_POINT=$(( READLINE_POINT + ${#text} ))
}
bind -x '"\ev": clipboard2path_paste'
"#
    .to_string()
}

fn generate_zsh_hook() -> String {
    r#"clipboard2path-paste() {
    local latest_path="$XDG_RUNTIME_DIR/clipboard2path/latest-path"
    if [[ -f "$latest_path" ]]; then
        local path
        path="$(cat "$latest_path")"
        if [[ -n "$path" ]]; then
            LBUFFER+="$path"
            return
        fi
    fi
    LBUFFER+="$(wl-paste -n 2>/dev/null)"
}
zle -N clipboard2path-paste
bindkey '\ev' clipboard2path-paste
"#
    .to_string()
}

/// Return the expected install path for the shell hook.
pub fn hook_install_path(shell: ShellType, home_dir: &str) -> String {
    match shell {
        ShellType::Fish => {
            format!("{home_dir}/.config/fish/conf.d/clipboard2path.fish")
        }
        ShellType::Bash => format!("{home_dir}/.bashrc"),
        ShellType::Zsh => format!("{home_dir}/.zshrc"),
    }
}

/// The marker comment used to identify our hook in .bashrc/.zshrc.
pub const HOOK_MARKER: &str = "# clipboard2path-wsl shell hook";

/// Generate the source line to add to .bashrc/.zshrc.
pub fn generate_source_line(hook_file: &str) -> String {
    format!("{HOOK_MARKER}\nsource \"{hook_file}\"\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fish_hook_contains_clipboard2path_function() {
        let hook = generate_hook(ShellType::Fish);
        assert!(hook.contains("function __clipboard2path_paste"));
        assert!(hook.contains("latest-path"));
        assert!(hook.contains("string trim"));
        assert!(hook.contains(r#"bind \ev"#));
    }

    #[test]
    fn bash_hook_contains_readline_binding() {
        let hook = generate_hook(ShellType::Bash);
        assert!(hook.contains("clipboard2path_paste"));
        assert!(hook.contains("READLINE_LINE"));
        assert!(hook.contains(r#"bind -x '"\ev": clipboard2path_paste'"#));
    }

    #[test]
    fn zsh_hook_contains_zle_widget() {
        let hook = generate_hook(ShellType::Zsh);
        assert!(hook.contains("clipboard2path-paste"));
        assert!(hook.contains("zle -N"));
        assert!(hook.contains(r#"bindkey '\ev'"#));
    }

    #[test]
    fn hook_install_path_fish() {
        let path = hook_install_path(ShellType::Fish, "/home/user");
        assert_eq!(path, "/home/user/.config/fish/conf.d/clipboard2path.fish");
    }

    #[test]
    fn hook_install_path_bash() {
        let path = hook_install_path(ShellType::Bash, "/home/user");
        assert_eq!(path, "/home/user/.bashrc");
    }

    #[test]
    fn hook_install_path_zsh() {
        let path = hook_install_path(ShellType::Zsh, "/home/user");
        assert_eq!(path, "/home/user/.zshrc");
    }

    #[test]
    fn source_line_contains_marker() {
        let line = generate_source_line("/path/to/hook.sh");
        assert!(line.contains(HOOK_MARKER));
        assert!(line.contains("source \"/path/to/hook.sh\""));
    }
}
