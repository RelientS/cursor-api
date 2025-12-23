use crate::common::utils::StringBuilder;
use std::{env, process::exit};

// String constants for natural language command parsing and user messages.
//
// Organized by usage context:
// - Token keywords for parser pattern matching
// - Default configuration values
// - User-facing message templates
crate::define_typed_constants! {
    pub &'static str => {
        // Token keywords
        TOKEN_IMPORT = "import",
        TOKEN_ENV = "env",
        TOKEN_FROM = "from",
        TOKEN_LISTEN = "listen",
        TOKEN_ON = "on",
        TOKEN_PORT = "port",
        TOKEN_HELP = "help",
        TOKEN_QUESTION = "?",
        TOKEN_OVERRIDE = "override",
        TOKEN_OVERRIDING = "overriding",
        TOKEN_AND = "and",
        TOKEN_EXISTING = "existing",

        // Parser vocabulary
        WORD_ME = "me",
        WORD_WHAT = "what",
        WORD_CAN = "can",
        WORD_YOU = "you",
        WORD_DO = "do",

        // Default values
        DEFAULT_ENV_FILE = ".env",
        DEFAULT_LISTEN_HOST = "0.0.0.0",
        DEFAULT_LISTEN_PORT = "3000",

        // Environment variable names
        ENV_HOST = "HOST",
        ENV_PORT = "PORT",

        // Delimiters
        COLON_SEPARATOR = ":",

        // Error message templates
        ERROR_NOT_UNDERSTAND = "I don't understand '",
        ERROR_TRY_HELP = "Try '",
        ERROR_TRY_HELP_SUFFIX = " help' to see what I can do\n\n",

        // Info message templates
        INFO_IMPORTING = "Importing environment from ",
        INFO_LOADED = " (loaded: ",
        INFO_SKIPPED = ", skipped: ",
        INFO_OVERRIDDEN = ", overridden: ",
        INFO_CLOSING = ")\n",
        INFO_STARTING = "Starting server on ",
    }
}

/// Tokenized representation of command-line input.
///
/// `String` and `Number` variants hold references to the original input,
/// avoiding string copies during tokenization.
#[derive(Debug, PartialEq, Clone, Copy)]
enum Token<'a> {
    Import,
    Env,
    From,
    Listen,
    On,
    Port,
    Help,
    Question,
    Override,
    Overriding,
    And,
    Existing,
    String(&'a str),
    Number(&'a str),
}

/// Parsed action to be executed.
///
/// Lifetime `'a` ensures references to command-line strings remain valid
/// throughout action execution.
#[derive(Debug)]
pub enum Action<'a> {
    ImportEnv { file: Option<&'a str>, override_existing: bool },
    Listen { host: Option<&'a str>, port: Option<&'a str> },
    Help,
}

/// Natural language command parser with borrowed string references.
///
/// # Design
///
/// - Tokens store `&'a str` references to avoid copying input strings
/// - Token vector requires one allocation, but enables flexible pattern matching
/// - Parsing uses slice patterns for clean command recognition
pub struct NaturalParser<'a> {
    tokens: Vec<Token<'a>>,
}

impl<'a> NaturalParser<'a> {
    /// Constructs parser from command-line arguments.
    ///
    /// Each argument is split by whitespace and tokenized. Token references
    /// point directly to the input `String` data (via `&'a str` slices).
    pub fn from_args(args: &'a [String]) -> Self {
        let mut tokens = Vec::with_capacity(args.len() + args.len() / 2);

        for arg in args {
            for word in arg.split_whitespace() {
                let token = match word {
                    TOKEN_IMPORT => Token::Import,
                    TOKEN_ENV => Token::Env,
                    TOKEN_FROM => Token::From,
                    TOKEN_LISTEN => Token::Listen,
                    TOKEN_ON => Token::On,
                    TOKEN_PORT => Token::Port,
                    TOKEN_HELP => Token::Help,
                    TOKEN_QUESTION => Token::Question,
                    TOKEN_OVERRIDE => Token::Override,
                    TOKEN_OVERRIDING => Token::Overriding,
                    TOKEN_AND => Token::And,
                    TOKEN_EXISTING => Token::Existing,
                    _ => {
                        if word.parse::<u16>().is_ok() {
                            Token::Number(word)
                        } else {
                            Token::String(word)
                        }
                    }
                };
                tokens.push(token);
            }
        }
        Self { tokens }
    }

    /// Parses token sequence into executable actions using slice pattern matching.
    ///
    /// Allocates action vector but avoids copying token string data.
    pub fn parse(&self) -> Vec<Action<'a>> {
        let mut actions = Vec::with_capacity(self.tokens.len() / 3);
        let mut i = 0;

        while i < self.tokens.len() {
            // SAFETY: Loop condition `i < self.tokens.len()` guarantees valid start index.
            // Each match arm advances `i` by the exact number of tokens consumed.
            // Slice patterns (`[..]` tail) always match remaining tokens without bounds check.
            match unsafe { self.tokens.get_unchecked(i..) } {
                [
                    Token::Import,
                    Token::Env,
                    Token::From,
                    Token::String(file),
                    Token::And,
                    Token::Override,
                    Token::Existing,
                    ..,
                ] => {
                    actions.push(Action::ImportEnv { file: Some(file), override_existing: true });
                    i += 7;
                }
                [Token::Import, Token::Env, Token::And, Token::Override, Token::Existing, ..] => {
                    actions.push(Action::ImportEnv { file: None, override_existing: true });
                    i += 5;
                }
                [Token::Import, Token::Env, Token::Overriding, Token::Existing, ..] => {
                    actions.push(Action::ImportEnv { file: None, override_existing: true });
                    i += 4;
                }
                [
                    Token::Import,
                    Token::Env,
                    Token::From,
                    Token::String(file),
                    Token::Overriding,
                    Token::Existing,
                    ..,
                ] => {
                    actions.push(Action::ImportEnv { file: Some(file), override_existing: true });
                    i += 6;
                }
                [Token::Import, Token::Env, Token::From, Token::String(file), ..] => {
                    actions.push(Action::ImportEnv { file: Some(file), override_existing: false });
                    i += 4;
                }
                [Token::Import, Token::Env, ..] => {
                    actions.push(Action::ImportEnv { file: None, override_existing: false });
                    i += 2;
                }
                [
                    Token::Listen,
                    Token::On,
                    Token::String(host),
                    Token::Port,
                    Token::Number(port_str),
                    ..,
                ] => {
                    actions.push(Action::Listen { host: Some(host), port: Some(port_str) });
                    i += 5;
                }
                [Token::Listen, Token::On, Token::Port, Token::Number(port_str), ..] => {
                    actions.push(Action::Listen { host: None, port: Some(port_str) });
                    i += 4;
                }
                // Handles "listen on <address>" where address can be:
                // - host:port (e.g., "localhost:8080")
                // - just host (e.g., "localhost")
                // - just port as number (e.g., "8080")
                [Token::Listen, Token::On, Token::String(addr), ..]
                | [Token::Listen, Token::On, Token::Number(addr), ..] => {
                    if let Some((host, port)) = addr.split_once(COLON_SEPARATOR) {
                        actions.push(Action::Listen { host: Some(host), port: Some(port) });
                    } else if matches!(self.tokens[i + 2], Token::Number(_)) {
                        actions.push(Action::Listen { host: None, port: Some(addr) });
                    } else {
                        actions.push(Action::Listen { host: Some(addr), port: None });
                    }
                    i += 3;
                }

                [Token::Help, Token::String(s), ..] if *s == WORD_ME => {
                    actions.push(Action::Help);
                    i += 2;
                }
                [Token::Help, ..] | [Token::Question, ..] => {
                    actions.push(Action::Help);
                    i += 1;
                }
                [
                    Token::String(w1),
                    Token::String(w2),
                    Token::String(w3),
                    Token::String(w4),
                    ..,
                ] if *w1 == WORD_WHAT && *w2 == WORD_CAN && *w3 == WORD_YOU && *w4 == WORD_DO => {
                    actions.push(Action::Help);
                    i += 4;
                }
                _ => i += 1,
            }
        }
        actions
    }
}

/// Displays help message and terminates the process.
///
/// Marked as cold path to optimize hot path code layout.
#[cold]
#[inline(never)]
fn handle_help_and_exit(program_name: &str) -> ! {
    print_help(program_name);
    exit(0);
}

/// Loads environment variables from file with optional override behavior.
///
/// Returns load statistics and prints informational message on success.
#[inline(always)]
fn load_env_file(filename: &str, override_existing: bool) {
    match if override_existing {
        dotenvy::from_filename_override(filename)
    } else {
        dotenvy::from_filename(filename)
    } {
        Ok(result) => {
            let mut buffer = itoa::Buffer::new();
            let mut msg = StringBuilder::with_capacity(7)
                .append(INFO_IMPORTING)
                .append(filename)
                .append(INFO_LOADED);

            if result.skipped_or_overridden > 0 {
                msg.append_mut(buffer.format(result.loaded).to_string());
                if override_existing {
                    msg.append_mut(INFO_OVERRIDDEN);
                } else {
                    msg.append_mut(INFO_SKIPPED);
                }
                msg.append_mut(buffer.format(result.skipped_or_overridden));
            } else {
                msg.append_mut(buffer.format(result.loaded));
            }

            msg.append_mut(INFO_CLOSING);
            __print!(msg.build());
        }
        Err(e) => {
            __cold_path!();
            eprintln!("Failed to load {filename}: {e}");
        }
    }
}

/// Processes command-line arguments as natural language commands.
///
/// When no arguments are provided, silently loads default `.env` file.
pub fn process_args(program_name: &str) {
    let args: Vec<String> = env::args_os()
        .skip(1)
        .map(|s| match s.into_string() {
            Ok(s) => s,
            Err(s) => s.to_string_lossy().into_owned(),
        })
        .collect();
    __process_args_impl(program_name, &args)
}

/// Internal implementation of argument processing.
///
/// Parses commands and executes corresponding actions (environment loading,
/// server configuration, or help display).
#[inline(always)]
fn __process_args_impl(program_name: &str, args: &[String]) {
    if args.is_empty() {
        load_env_file(DEFAULT_ENV_FILE, false);
        return;
    }

    let parser = NaturalParser::from_args(args);
    let actions = parser.parse();

    if actions.is_empty() {
        __cold_path!();
        let command = args.join(" ");
        __eprint!(
            StringBuilder::with_capacity(6)
                .append(ERROR_NOT_UNDERSTAND)
                .append(&command)
                .append("'\n")
                .append(ERROR_TRY_HELP)
                .append(program_name)
                .append(ERROR_TRY_HELP_SUFFIX)
                .build()
        );
        return;
    }

    for action in actions {
        match action {
            Action::ImportEnv { file, override_existing } => {
                let env_file = file.unwrap_or(DEFAULT_ENV_FILE);
                load_env_file(env_file, override_existing);
            }
            Action::Listen { host, port } => {
                let h = host.unwrap_or(DEFAULT_LISTEN_HOST);
                let p = port.unwrap_or(DEFAULT_LISTEN_PORT);

                __print!(
                    StringBuilder::with_capacity(5)
                        .append(INFO_STARTING)
                        .append(h)
                        .append(":")
                        .append(p)
                        .append("\n")
                        .build()
                );

                // SAFETY:
                // 1. [Lifetime] Slices `h` and `p` are valid static or process-lifetime refs
                // 2. [Concurrency] Assumes single-threaded execution before server start
                //    - On Unix: Undefined behavior if called after thread spawn
                //    - Safe on Windows
                //    - Caller MUST ensure this runs before tokio/thread initialization
                unsafe {
                    env::set_var(ENV_HOST, h);
                    env::set_var(ENV_PORT, p);
                }
            }
            Action::Help => handle_help_and_exit(program_name),
        }
    }
}

/// Prints comprehensive help message showing all supported command patterns.
#[cold]
#[inline(never)]
fn print_help(program: &str) {
    println!(
        "
Hi! I'm {program}, and here's what I understand:

📦 Environment stuff:
   {program} import env                                      Load from default .env file
   {program} import env from config.env                      Load environment from a specific file
   {program} import env and override existing                Override existing vars from .env
   {program} import env overriding existing                  Alternative syntax for override
   {program} import env from prod.env and override existing  Override from specific file
   {program} import env from prod.env overriding existing    Alternative syntax

🌐 Server stuff:  
   {program} listen on 127.0.0.1 port 8080                   Listen on specific IP and port
   {program} listen on localhost port 3000                   Listen on localhost with port
   {program} listen on port 8080                             Listen on all interfaces (0.0.0.0)
   {program} listen on 192.168.1.1:8080                      IP:port format
   {program} listen on 8080                                  Just the port (defaults to 0.0.0.0)
   {program} listen on localhost                             Just the host (defaults to port 3000)

❓ Getting help:  
   {program} help                                            Show this message
   {program} help me                                         Same thing, but more polite
   {program} ?                                               Quick help
   {program} what can you do                                 Natural language help

Examples:
   {program} import env from .env.prod and override existing listen on 10.0.0.1 port 8080
   {program} listen on localhost:5000 import env overriding existing
"
    );
}
