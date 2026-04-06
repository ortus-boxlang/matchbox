#[cfg(feature = "bif-cli")]
use crate::types::{BxVM, BxValue};
#[cfg(feature = "bif-cli")]
use std::io::{self, Write};

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

#[cfg(feature = "bif-cli")]
pub fn cli_get_args(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let all_args = vm.get_cli_args();
    let options_id = vm.struct_new();
    let positionals_id = vm.array_new();

    let mut user_args = Vec::new();
    // Always skip the first argument (executable path)
    if !all_args.is_empty() {
        let mut skip = true;
        for arg in all_args {
            if skip {
                skip = false;
                continue;
            }
            user_args.push(arg);
        }
    }

    for arg in user_args {
        if arg.starts_with("--") {
            let part = &arg[2..];
            if part.starts_with('!') {
                vm.struct_set(options_id, &part[1..], BxValue::new_bool(false));
            } else if part.starts_with("no-") {
                vm.struct_set(options_id, &part[3..], BxValue::new_bool(false));
            } else if let Some(idx) = part.find('=') {
                let key = &part[..idx];
                let val = &part[idx + 1..];
                let val_id = vm.string_new(val.to_string());
                vm.struct_set(options_id, key, BxValue::new_ptr(val_id));
            } else {
                vm.struct_set(options_id, part, BxValue::new_bool(true));
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            let part = &arg[1..];
            if let Some(idx) = part.find('=') {
                let key = &part[..idx];
                let val = &part[idx + 1..];
                let val_id = vm.string_new(val.to_string());
                vm.struct_set(options_id, key, BxValue::new_ptr(val_id));
            } else {
                vm.struct_set(options_id, part, BxValue::new_bool(true));
            }
        } else {
            let s_id = vm.string_new(arg);
            vm.array_push(positionals_id, BxValue::new_ptr(s_id));
        }
    }

    let result_id = vm.struct_new();
    vm.struct_set(result_id, "options", BxValue::new_ptr(options_id));
    vm.struct_set(result_id, "positionals", BxValue::new_ptr(positionals_id));

    Ok(BxValue::new_ptr(result_id))
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
            terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType},
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
