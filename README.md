# Yazelix Zellij Popup

Yazelix Zellij Popup is a small Zellij plugin for toggling configured floating TUI popups. The Zellij plugin alias and wasm artifact are `yzpp`.

It gives plain Zellij users the popup behavior that Yazelix uses for tools like git UIs: press one key to open the floating pane, press it again from another pane to focus it, press it while the popup is focused to close it.

## Install

Build the plugin:

```bash
nix build .#yazelix-zellij-popup
```

`.#yzpp` is also provided as a short package alias for the plugin artifact.

The package installs:

- `share/yazelix_zellij_popup/yzpp.wasm`
- `share/yazelix_zellij_popup/examples/gitui.kdl`
- `share/yazelix_zellij_popup/examples/gitui.template.kdl`

## Configure

Add the plugin and a popup spec to your Zellij config:

```kdl
plugins {
    yzpp location="file:/path/to/yzpp.wasm" {
        popup {
            command "gitui"
            pane_title "gitui_popup"
            command_marker "gitui"
            cwd "."
            width_percent 90
            height_percent 85
        }
    }
}

load_plugins {
    yzpp
}

keybinds {
    normal {
        bind "Alt g" {
            MessagePlugin "yzpp" {
                name "toggle"
            }
        }
    }
}
```

The message `name` is the action. Supported actions are `toggle`, `open`, `focus`, and `close`. When there is only one configured popup or a `default` popup, no payload is needed.

## Popup Specs

The default popup uses a nested `popup` block:

```kdl
popup {
    command "gitui"
    arg_1 "--watch"
    pane_title "gitui_popup"
    command_marker "gitui"
    cwd "."
    width_percent 90
    height_percent 85
}
```

Required:

- `command`

Optional:

- `arg_1`, `arg_2`, and so on for argv arguments
- `pane_title`, defaulting to `default_popup`
- `command_marker`, defaulting to the command path
- `cwd`, defaulting to the focused terminal pane cwd; relative values resolve against that focused cwd
- `on_close`, an optional command hook run when `yzpp` closes the popup through `toggle` or `close`
- `toggle_close_behavior`, either `close` or `hide`, defaulting to `close`
- `width_percent`, defaulting to `90`
- `height_percent`, defaulting to `85`

Width and height must be integers from `1` through `100`. Commands are argv, not shell strings.

Hooks are also argv, not shell strings:

```kdl
popup {
    command "lazygit"
    pane_title "lazygit_popup"
    on_close {
        command "yzx"
        arg_1 "sidebar"
        arg_2 "refresh"
    }
}
```

`on_close` runs only when `yzpp` closes the pane in response to `toggle` or `close`. It does not run when the child process exits on its own.

Use `toggle_close_behavior "hide"` for monitor TUIs that should keep process state between toggles:

```kdl
popup {
    command "btm"
    pane_title "btm_popup"
    toggle_close_behavior "hide"
}
```

With `hide`, pressing the toggle key while the popup is focused hides the floating layer without killing the popup process. Pressing the toggle key again focuses the existing pane. The explicit `close` action still closes the pane and runs `on_close`.

For multiple popups in the same plugin config, use a nested `popups` block and send the popup id as the payload:

```kdl
popups {
    gitui {
        command "gitui"
        pane_title "gitui_popup"
        width_percent 90
        height_percent 85
    }

    lazygit {
        command "lazygit"
        pane_title "lazygit_popup"
        width_percent 92
        height_percent 88
    }
}
```

```kdl
MessagePlugin "yzpp" {
    name "toggle"
    payload "lazygit"
}
```

## Raw Pipe API

Generated integrations may still send the raw JSON request shape through `name "transient_popup"`:

```kdl
MessagePlugin "yzpp" {
    name "transient_popup"
    payload "{\"action\":\"toggle\",\"spec\":{\"id\":\"gitui\",\"pane_title\":\"gitui_popup\",\"command_marker\":\"gitui\",\"command\":[\"gitui\"],\"cwd\":\".\",\"width_percent\":90,\"height_percent\":85},\"args\":[]}"
}
```

That raw path exists for generated callers. Human-authored Zellij config should prefer configured popup specs plus `name "toggle"`.

## Permissions

Zellij prompts for plugin permissions when the plugin first loads. `yzpp` requests:

- `ReadApplicationState`
- `ChangeApplicationState`
- `OpenTerminalsOrPlugins`
- `RunCommands`
- `ReadCliPipes`

These permissions cover pane discovery, opening command panes, focusing and closing the managed pane, and receiving `MessagePlugin` pipe requests.

## Verify

```bash
cargo test
nix build
```
