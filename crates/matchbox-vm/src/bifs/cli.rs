use crate::types::{BxVM, BxValue};
#[cfg(feature = "bif-cli")]
use std::io::{self, IsTerminal, Write};

#[cfg(feature = "bif-cli")]
pub fn cli_clear(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        print!("\x1B[2J\x1B[1;1H");
        let _ = io::stdout().flush();
    }
    Ok(BxValue::new_null())
}

#[cfg(feature = "bif-cli")]
pub fn cli_exit(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let code = if args.len() >= 1 && args[0].is_number() {
        args[0].as_number() as i32
    } else {
        0
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::process::exit(code);

    #[cfg(target_arch = "wasm32")]
    return Err("cliExit not supported in WASM environment".to_string());

    #[allow(unreachable_code)]
    Ok(BxValue::new_null())
}

/// Value of a parsed command line option.
#[derive(Debug, Clone, PartialEq)]
pub enum CliOptionValue {
    Flag(bool),
    Text(String),
}

fn clean_quoted_value(value: &str) -> &str {
    let quoted = |quote: char| value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote);
    if quoted('"') || quoted('\'') {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

/// Port of BoxLang's `CLIUtil.parseArguments`: `--option`, `--option=value` (optionally quoted),
/// `--!option` / `--no-option` (false), `-o`, `-o=value`, combined `-abc`; everything else is
/// positional. A value is only ever attached with `=`, and a repeated option overrides the earlier one.
pub fn parse_cli_arguments(args: &[String]) -> (Vec<(String, CliOptionValue)>, Vec<String>) {
    let mut options: Vec<(String, CliOptionValue)> = Vec::new();
    let mut positionals = Vec::new();
    let mut put = |key: String, value: CliOptionValue| {
        options.retain(|(existing, _)| !existing.eq_ignore_ascii_case(&key));
        options.push((key, value));
    };
    for arg in args {
        if let Some(rest) = arg.strip_prefix("--") {
            if let Some(negated) = arg.strip_prefix("--!").or_else(|| arg.strip_prefix("--no-")) {
                put(negated.trim().to_string(), CliOptionValue::Flag(false));
            } else {
                match rest.split_once('=') {
                    None => put(rest.trim().to_string(), CliOptionValue::Flag(true)),
                    Some((key, value)) => put(
                        key.trim().to_string(),
                        CliOptionValue::Text(clean_quoted_value(value.trim()).to_string()),
                    ),
                }
            }
        } else if arg.starts_with('-') && arg.chars().count() > 1 {
            let chars: Vec<char> = arg.chars().collect();
            for i in 1..chars.len() {
                if i == 1 && chars.len() > 2 && chars[2] == '=' {
                    let value: String = chars[3..].iter().collect();
                    put(chars[1].to_string(), CliOptionValue::Text(clean_quoted_value(value.trim()).to_string()));
                    break;
                }
                put(chars[i].to_string(), CliOptionValue::Flag(true));
            }
        } else {
            positionals.push(arg.clone());
        }
    }
    (options, positionals)
}

/// `{ options, positionals }` for the given user arguments (the executable is not included).
pub fn parsed_arguments_value(vm: &mut dyn BxVM, user_args: &[String]) -> BxValue {
    let (options, positionals) = parse_cli_arguments(user_args);

    let options_id = vm.struct_new();
    for (key, value) in options {
        let value = match value {
            CliOptionValue::Flag(flag) => BxValue::new_bool(flag),
            CliOptionValue::Text(text) => BxValue::new_ptr(vm.string_new(text)),
        };
        vm.struct_set(options_id, &key, value);
    }
    let positionals_id = vm.array_new();
    for positional in positionals {
        let value = BxValue::new_ptr(vm.string_new(positional));
        vm.array_push(positionals_id, value);
    }

    let result_id = vm.struct_new();
    vm.struct_set(result_id, "options", BxValue::new_ptr(options_id));
    vm.struct_set(result_id, "positionals", BxValue::new_ptr(positionals_id));

    BxValue::new_ptr(result_id)
}

#[cfg(feature = "bif-cli")]
pub fn cli_get_args(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    // The first entry is the executable.
    let user_args: Vec<String> = vm.get_cli_args().into_iter().skip(1).collect();
    Ok(parsed_arguments_value(vm, &user_args))
}

#[cfg(feature = "bif-cli")]
pub fn cli_read(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() >= 1 {
        print!("{}", vm.to_string(args[0]));
        let _ = io::stdout().flush();
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                let trimmed = input.trim_end_matches(['\r', '\n']).to_string();
                Ok(BxValue::new_ptr(vm.string_new(trimmed)))
            }
            Err(e) => Err(format!("Failed to read from stdin: {}", e)),
        }
    }

    #[cfg(target_arch = "wasm32")]
    Err("cliRead not supported in WASM environment".to_string())
}

#[cfg(feature = "bif-cli")]
pub fn cli_is_terminal(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    #[cfg(not(target_arch = "wasm32"))]
    return Ok(BxValue::new_bool(io::stdin().is_terminal()));

    #[cfg(target_arch = "wasm32")]
    Ok(BxValue::new_bool(false))
}

#[cfg(feature = "bif-cli")]
pub fn cli_read_key(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "espidf")))]
    {
        use crossterm::{
            event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
            terminal::{disable_raw_mode, enable_raw_mode},
        };

        if !io::stdin().is_terminal() {
            return Ok(BxValue::new_ptr(vm.string_new(String::new())));
        }

        enable_raw_mode().map_err(|error| error.to_string())?;
        let result = (|| -> Result<String, String> {
            loop {
                if let Event::Key(KeyEvent {
                    code,
                    modifiers,
                    kind,
                    ..
                }) = event::read().map_err(|error| error.to_string())?
                {
                    if kind == KeyEventKind::Release {
                        continue;
                    }
                    return Ok(match code {
                        KeyCode::Left => "left".to_string(),
                        KeyCode::Right => "right".to_string(),
                        KeyCode::Up => "up".to_string(),
                        KeyCode::Down => "down".to_string(),
                        KeyCode::Enter => "enter".to_string(),
                        KeyCode::Esc => "escape".to_string(),
                        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                            "ctrl-c".to_string()
                        }
                        KeyCode::Char(character)
                            if !modifiers.intersects(
                                KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                            ) =>
                        {
                            character.to_string()
                        }
                        _ => continue,
                    });
                }
            }
        })();
        disable_raw_mode().map_err(|error| error.to_string())?;
        return result.map(|key| BxValue::new_ptr(vm.string_new(key)));
    }

    #[cfg(any(target_arch = "wasm32", target_os = "espidf"))]
    Ok(BxValue::new_ptr(vm.string_new(String::new())))
}

#[cfg(feature = "bif-cli")]
pub fn cli_read_password(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let prompt = args
        .first()
        .map(|value| vm.to_string(*value))
        .unwrap_or_else(|| "Secret value: ".to_string());

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "espidf")))]
    {
        use crossterm::{
            event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
            terminal::{disable_raw_mode, enable_raw_mode},
        };

        enable_raw_mode().map_err(|error| error.to_string())?;
        print!("{prompt}");
        let _ = io::stdout().flush();
        let result = (|| -> Result<String, String> {
            let mut input = String::new();
            loop {
                if let Event::Key(KeyEvent {
                    code,
                    modifiers,
                    kind,
                    ..
                }) = event::read().map_err(|error| error.to_string())?
                {
                    if kind == KeyEventKind::Release {
                        continue;
                    }
                    match code {
                        KeyCode::Enter => return Ok(input),
                        KeyCode::Backspace => {
                            input.pop();
                        }
                        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                            return Err("Secret input cancelled".to_string());
                        }
                        KeyCode::Char(character)
                            if !modifiers.intersects(
                                KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                            ) =>
                        {
                            input.push(character);
                        }
                        _ => {}
                    }
                }
            }
        })();
        disable_raw_mode().map_err(|error| error.to_string())?;
        println!();
        let input = result?;
        return Ok(BxValue::new_ptr(vm.string_new(input)));
    }

    #[cfg(any(target_arch = "wasm32", target_os = "espidf"))]
    Err("cliReadPassword is not supported on this target".to_string())
}

#[cfg(feature = "bif-cli")]
pub fn cli_confirm(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let prompt = if args.len() >= 1 {
        vm.to_string(args[0])
    } else {
        "Confirm?".to_string()
    };

    print!("{} (Y/n): ", prompt);
    let _ = io::stdout().flush();

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                let trimmed = input.trim().to_lowercase();
                Ok(BxValue::new_bool(
                    trimmed == "y" || trimmed == "yes" || trimmed.is_empty(),
                ))
            }
            Err(e) => Err(format!("Failed to read from stdin: {}", e)),
        }
    }

    #[cfg(target_arch = "wasm32")]
    Err("cliConfirm not supported in WASM environment".to_string())
}

#[cfg(feature = "bif-cli")]
pub fn cli_select(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("cliSelect() expects 2 arguments: (title, options_array)".to_string());
    }
    let title = vm.to_string(args[0]);
    let options_id = args[1]
        .as_gc_id()
        .ok_or("Second argument must be an array of options")?;
    let len = vm.array_len(options_id);
    let mut options = Vec::with_capacity(len);
    for i in 0..len {
        options.push(vm.to_string(vm.array_get(options_id, i)));
    }

    if options.is_empty() {
        return Ok(BxValue::new_ptr(vm.string_new("".to_string())));
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        use crossterm::{
            cursor::{Hide, MoveToColumn, MoveUp, Show},
            event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
            execute,
            style::{Color, Print, ResetColor, SetForegroundColor},
            terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
        };
        use std::io::stdout;

        let mut selected = 0;
        let mut stdout = stdout();

        enable_raw_mode().map_err(|e| e.to_string())?;
        execute!(stdout, Hide).map_err(|e| e.to_string())?;

        let result = (|| -> Result<String, String> {
            loop {
                // Render
                execute!(stdout, Clear(ClearType::CurrentLine), MoveToColumn(0))
                    .map_err(|e| e.to_string())?;
                println!("{} (Arrows to navigate, Enter to select):", title);

                for (i, opt) in options.iter().enumerate() {
                    execute!(stdout, Clear(ClearType::CurrentLine), MoveToColumn(0))
                        .map_err(|e| e.to_string())?;
                    if i == selected {
                        execute!(
                            stdout,
                            SetForegroundColor(Color::Cyan),
                            Print("> "),
                            Print(opt),
                            ResetColor
                        )
                        .map_err(|e| e.to_string())?;
                    } else {
                        print!("  {}", opt);
                    }
                    println!();
                }

                // Input
                if let Event::Key(KeyEvent {
                    code,
                    modifiers,
                    kind,
                    ..
                }) = event::read().map_err(|e| e.to_string())?
                {
                    if kind == KeyEventKind::Release {
                        continue;
                    }
                    match code {
                        KeyCode::Up => {
                            if selected > 0 {
                                selected -= 1;
                            }
                            execute!(stdout, MoveUp((options.len() + 1) as u16))
                                .map_err(|e| e.to_string())?;
                        }
                        KeyCode::Down => {
                            if selected < options.len() - 1 {
                                selected += 1;
                            }
                            execute!(stdout, MoveUp((options.len() + 1) as u16))
                                .map_err(|e| e.to_string())?;
                        }
                        KeyCode::Enter => {
                            // Clear the menu before returning
                            execute!(stdout, MoveUp((options.len() + 1) as u16))
                                .map_err(|e| e.to_string())?;
                            for _ in 0..=options.len() {
                                execute!(stdout, Clear(ClearType::CurrentLine))
                                    .map_err(|e| e.to_string())?;
                                println!();
                            }
                            execute!(stdout, MoveUp((options.len() + 1) as u16))
                                .map_err(|e| e.to_string())?;
                            return Ok(options[selected].clone());
                        }
                        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                            return Err("Selection cancelled".to_string());
                        }
                        _ => {
                            execute!(stdout, MoveUp((options.len() + 1) as u16))
                                .map_err(|e| e.to_string())?;
                        }
                    }
                }
            }
        })();

        let _ = execute!(stdout, Show);
        let _ = disable_raw_mode();

        match result {
            Ok(s) => Ok(BxValue::new_ptr(vm.string_new(s))),
            Err(e) => Err(e),
        }
    }

    #[cfg(target_arch = "wasm32")]
    Err("cliSelect not supported in WASM environment".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use CliOptionValue::{Flag, Text};

    fn parse(args: &[&str]) -> (Vec<(String, CliOptionValue)>, Vec<String>) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        parse_cli_arguments(&args)
    }

    fn option(options: &[(String, CliOptionValue)], key: &str) -> Option<CliOptionValue> {
        options.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn boxlang_documented_example() {
        let (options, positionals) = parse(&["--debug", "--!verbose", "--config=prod.json", "-o='/path/to/file'", "-v", "my/path/template"]);
        assert_eq!(option(&options, "debug"), Some(Flag(true)));
        assert_eq!(option(&options, "verbose"), Some(Flag(false)));
        assert_eq!(option(&options, "config"), Some(Text("prod.json".into())));
        assert_eq!(option(&options, "o"), Some(Text("/path/to/file".into())));
        assert_eq!(option(&options, "v"), Some(Flag(true)));
        assert_eq!(positionals, ["my/path/template"]);
    }

    #[test]
    fn cli_scripting_docs_example() {
        let (options, positionals) = parse(&["hola", "luis=majano", "--test"]);
        assert_eq!(positionals, ["hola", "luis=majano"]);
        assert_eq!(options, [("test".to_string(), Flag(true))]);
    }

    #[test]
    fn values_are_only_attached_with_equals() {
        let (options, positionals) = parse(&["--name", "luis"]);
        assert_eq!(option(&options, "name"), Some(Flag(true)));
        assert_eq!(positionals, ["luis"]);
    }

    #[test]
    fn negation_quotes_and_combined_shorthand() {
        let (options, _) = parse(&["--no-debug", "--message=\"Hello World\"", "--single='a b'", "-abc"]);
        assert_eq!(option(&options, "debug"), Some(Flag(false)));
        assert_eq!(option(&options, "message"), Some(Text("Hello World".into())));
        assert_eq!(option(&options, "single"), Some(Text("a b".into())));
        for key in ["a", "b", "c"] {
            assert_eq!(option(&options, key), Some(Flag(true)));
        }
    }

    #[test]
    fn repeated_options_override_and_a_lone_dash_is_positional() {
        let (options, positionals) = parse(&["--x=1", "--x=2", "-"]);
        assert_eq!(options, [("x".to_string(), Text("2".into()))]);
        assert_eq!(positionals, ["-"]);
    }
}
