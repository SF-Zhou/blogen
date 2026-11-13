//! Command line interface for the `blogen` binary.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::config::LoadedConfig;
use crate::error::Result;
use crate::theme::Theme;
use crate::{init, output, server, site};

/// A fast, themeable static blog generator.
#[derive(Debug, Parser)]
#[command(
    name = "blogen",
    version,
    about = "A fast, themeable static blog generator",
    long_about = None,
    propagate_version = true
)]
pub struct Cli {
    /// Path to the configuration file (defaults to `blogen.toml`, searched
    /// upwards from the current directory).
    #[arg(short, long, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Increase logging verbosity (repeatable).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Only print errors.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Sub-command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Available sub-commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Generate the static site.
    Build(BuildArgs),
    /// Create a new site skeleton.
    Init(InitArgs),
    /// Build and serve the site locally, rebuilding on changes.
    Serve(ServeArgs),
    /// Remove generated files.
    Clean(CleanArgs),
    /// Theme utilities.
    Theme(ThemeArgs),
}

/// Arguments for `blogen build`.
#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Override the output directory from the configuration.
    #[arg(short, long, value_name = "DIR")]
    pub output: Option<PathBuf>,

    /// Keep stale files that are no longer generated.
    #[arg(long)]
    pub no_clean: bool,

    /// Treat build warnings as errors.
    #[arg(long)]
    pub strict: bool,
}

/// Arguments for `blogen init`.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Target directory.
    #[arg(default_value = ".")]
    pub dir: PathBuf,

    /// Site name (defaults to the directory name).
    #[arg(long)]
    pub name: Option<String>,

    /// Canonical site URL.
    #[arg(long)]
    pub url: Option<String>,

    /// Overwrite existing files.
    #[arg(short, long)]
    pub force: bool,

    /// Do not create the example article.
    #[arg(long)]
    pub no_example: bool,
}

/// Arguments for `blogen serve`.
#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Address to bind (overrides `[serve].host`).
    #[arg(long)]
    pub host: Option<String>,

    /// Port to bind (overrides `[serve].port`).
    #[arg(short, long)]
    pub port: Option<u16>,

    /// Do not rebuild when files change.
    #[arg(long)]
    pub no_watch: bool,

    /// Override the output directory from the configuration.
    #[arg(short, long, value_name = "DIR")]
    pub output: Option<PathBuf>,
}

/// Arguments for `blogen clean`.
#[derive(Debug, Args)]
pub struct CleanArgs {
    /// Remove the whole output directory instead of only generated files.
    #[arg(long)]
    pub force: bool,
}

/// Arguments for `blogen theme`.
#[derive(Debug, Args)]
pub struct ThemeArgs {
    /// Theme sub-command.
    #[command(subcommand)]
    pub command: ThemeCommand,
}

/// Theme sub-commands.
#[derive(Debug, Subcommand)]
pub enum ThemeCommand {
    /// List bundled themes.
    List,
    /// Write a bundled theme to a directory for customisation.
    Extract {
        /// Bundled theme name.
        name: String,
        /// Target directory.
        dir: PathBuf,
    },
}

/// Run the CLI.
pub fn run(cli: Cli) -> Result<()> {
    let Cli {
        config,
        quiet,
        command,
        ..
    } = cli;
    match command {
        Command::Build(args) => run_build(config.as_deref(), quiet, &args),
        Command::Init(args) => run_init(args),
        Command::Serve(args) => run_serve(config.as_deref(), args),
        Command::Clean(args) => run_clean(config.as_deref(), quiet, &args),
        Command::Theme(args) => run_theme(args),
    }
}

fn run_build(config: Option<&std::path::Path>, quiet: bool, args: &BuildArgs) -> Result<()> {
    let mut loaded = LoadedConfig::discover(config)?;
    if let Some(output) = &args.output {
        loaded.override_output(output);
    }
    if args.no_clean {
        loaded.config.build.clean = false;
    }
    let report = site::build(&loaded)?;
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    if args.strict && !report.warnings.is_empty() {
        return Err(crate::error::Error::Cli(format!(
            "{} build warning(s) treated as errors because of `--strict`",
            report.warnings.len()
        )));
    }
    if !quiet {
        println!(
            "Built {} article(s) ({} hidden) in {:.2}s: {} written, {} unchanged, {} removed",
            report.visible_articles,
            report.hidden_articles,
            report.duration.as_secs_f64(),
            report.output.written,
            report.output.unchanged,
            report.output.removed,
        );
        println!("Output: {}", loaded.output_dir().display());
    }
    Ok(())
}

fn run_init(args: InitArgs) -> Result<()> {
    let options = init::InitOptions {
        dir: args.dir.clone(),
        name: args.name,
        url: args.url,
        force: args.force,
        example: !args.no_example,
    };
    let created = init::init(&options)?;
    println!("Created a new blogen site in `{}`: ", args.dir.display());
    for path in &created {
        println!("  {}", path.display());
    }
    println!("\nNext steps:");
    println!("  cd {}", args.dir.display());
    println!("  blogen serve");
    Ok(())
}

fn run_serve(config: Option<&std::path::Path>, args: ServeArgs) -> Result<()> {
    let mut loaded = LoadedConfig::discover(config)?;
    if let Some(output) = &args.output {
        loaded.override_output(output);
    }
    let options = server::ServeOptions {
        host: args
            .host
            .unwrap_or_else(|| loaded.config.serve.host.clone()),
        port: args.port.unwrap_or(loaded.config.serve.port),
        watch: !args.no_watch,
        output_override: args.output,
    };
    server::serve(&loaded, &options)
}

fn run_clean(config: Option<&std::path::Path>, quiet: bool, args: &CleanArgs) -> Result<()> {
    let loaded = LoadedConfig::discover(config)?;
    let output_dir = loaded.output_dir();
    let removed = output::clean_output(&output_dir, args.force)?;
    if !quiet {
        if args.force {
            println!("Removed the output directory `{}`", output_dir.display());
        } else {
            println!(
                "Removed {removed} generated file(s) from `{}`",
                output_dir.display()
            );
        }
    }
    Ok(())
}

fn run_theme(args: ThemeArgs) -> Result<()> {
    match args.command {
        ThemeCommand::List => {
            let themes = Theme::bundled_manifests();
            if themes.is_empty() {
                println!("No bundled themes.");
                return Ok(());
            }
            for theme in themes {
                println!(
                    "{} {} - {}",
                    theme.name,
                    theme.version,
                    if theme.description.is_empty() {
                        "(no description)"
                    } else {
                        &theme.description
                    }
                );
            }
            Ok(())
        }
        ThemeCommand::Extract { name, dir } => {
            let theme = Theme::load_bundled(&name)?;
            theme.extract(&dir)?;
            println!(
                "Extracted theme `{}` to `{}`.\nUse it with:\n\n[theme]\npath = \"{}\"\n",
                theme.manifest().name,
                dir.display(),
                dir.display()
            );
            Ok(())
        }
    }
}
