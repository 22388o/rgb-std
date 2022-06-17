// RGB Standard Library: high-level API to RGB smart contracts.
// Written in 2019-2022 by
//     Dr. Maxim Orlovsky <orlovsky@lnp-bp.org>
//
// To the extent possible under law, the author(s) have dedicated all copyright
// and related and neighboring rights to this software to the public domain
// worldwide. This software is distributed without any warranty.
//
// You should have received a copy of the MIT License along with this software.
// If not, see <https://opensource.org/licenses/MIT>.

#[macro_use]
extern crate clap;
#[macro_use]
extern crate amplify;
extern crate serde_crate as serde;

use std::fmt::{Debug, Display};
use std::io::{self, Read};
use std::str::FromStr;

use amplify::hex::{self, FromHex, ToHex};
use clap::Parser;
use commit_verify::ConsensusCommit;
use rgb::{Disclosure, Extension, Genesis, Schema, Transition};
use serde::Serialize;
use strict_encoding::{StrictDecode, StrictEncode};

#[derive(Parser, Clone, Debug)]
#[clap(
    name = "rgb",
    bin_name = "rgb",
    author,
    version,
    about = "Command-line tool for working with RGB smart contracts"
)]
pub struct Opts {
    /// Command to execute
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Command {
    /// Commands for working with consignments
    Consignment {
        #[clap(subcommand)]
        subcommand: ConsignmentCommand,
    },

    /// Commands for working with disclosures
    Disclosure {
        #[clap(subcommand)]
        subcommand: DisclosureCommand,
    },

    /// Commands for working with schemata
    Schema {
        #[clap(subcommand)]
        subcommand: SchemaCommand,
    },

    /// Commands for working with anchors and multi-message commitments
    Anchor {
        #[clap(subcommand)]
        subcommand: AnchorCommand,
    },

    /// Commands for working with state extensions
    Extension {
        #[clap(subcommand)]
        subcommand: ExtensionCommand,
    },

    /// Commands for working with state transitions
    Transition {
        #[clap(subcommand)]
        subcommand: TransitionCommand,
    },

    /// Commands for working with contract geneses
    Genesis {
        #[clap(subcommand)]
        subcommand: GenesisCommand,
    },
}

#[derive(Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ConsignmentCommand {
    Validate {
        /// Consignment data; if none are given reads from STDIN
        consignment: Option<String>,

        /// Schema string (in a Bech32 format). Defaults to RGB20 schema if
        /// omitted.
        schema: Option<String>,

        /// Formatting of the input data
        #[clap(short, long, default_value = "bech32")]
        input: Format,

        /// Address for Electrum server
        #[clap(default_value = "pandora.network:60001")]
        electrum: String,
    },
}

#[derive(Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum DisclosureCommand {
    Convert {
        /// Consignment data; if none are given reads from STDIN
        disclosure: Option<String>,

        /// Formatting of the input data
        #[clap(short, long, default_value = "bech32")]
        input: Format,

        /// Formatting for the output
        #[clap(short, long, default_value = "yaml")]
        output: Format,
    },
}

#[derive(Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum SchemaCommand {
    Convert {
        /// Schema data; if none are given reads from STDIN
        schema: Option<String>,

        /// Formatting of the input data
        #[clap(short, long, default_value = "bech32")]
        input: Format,

        /// Formatting for the output
        #[clap(short, long, default_value = "yaml")]
        output: Format,
    },
}

#[derive(Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum AnchorCommand {}

#[derive(Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ExtensionCommand {
    Convert {
        /// State extension data; if none are given reads from STDIN
        extension: Option<String>,

        /// Formatting of the input data
        #[clap(short, long, default_value = "bech32")]
        input: Format,

        /// Formatting for the output
        #[clap(short, long, default_value = "yaml")]
        output: Format,
    },
}

#[derive(Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum TransitionCommand {
    Convert {
        /// State transition data; if none are given reads from STDIN
        transition: Option<String>,

        /// Formatting of the input data
        #[clap(short, long, default_value = "bech32")]
        input: Format,

        /// Formatting for the output
        #[clap(short, long, default_value = "yaml")]
        output: Format,
    },
}

#[derive(Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum GenesisCommand {
    Convert {
        /// Genesis data; if none are given reads from STDIN
        genesis: Option<String>,

        /// Formatting of the input data
        #[clap(short, long, default_value = "bech32")]
        input: Format,

        /// Formatting for the output
        #[clap(short, long, default_value = "yaml")]
        output: Format,
    },
}

#[derive(ArgEnum, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Display)]
pub enum Format {
    /// Format according to the rust debug rules
    #[display("debug")]
    Debug,

    /// Format according to default display formatting
    #[display("bech32")]
    Bech32,

    /// Format as YAML
    #[display("yaml")]
    Yaml,

    /// Format as JSON
    #[display("json")]
    Json,

    /// Format according to the strict encoding rules
    #[display("hex")]
    Hexadecimal,

    /// Format as a rust array (using hexadecimal byte values)
    #[display("rust")]
    Rust,

    /// Produce binary (raw) output
    #[display("raw")]
    Binary,

    /// Produce client-validated commitment
    #[display("commitment")]
    Commitment,
}

impl FromStr for Format {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim().to_lowercase().as_str() {
            "debug" => Format::Debug,
            "bech32" => Format::Bech32,
            "yaml" => Format::Yaml,
            "json" => Format::Json,
            "hex" => Format::Hexadecimal,
            "raw" | "bin" | "binary" => Format::Binary,
            "rust" => Format::Rust,
            "commitment" => Format::Commitment,
            other => Err(format!("Unknown format: {}", other))?,
        })
    }
}

fn input_read<T>(data: Option<String>, format: Format) -> Result<T, String>
where T: StrictDecode + for<'de> serde::Deserialize<'de> {
    let data = data
        .map(|d| d.as_bytes().to_vec())
        .ok_or_else(String::new)
        .or_else(|_| -> Result<Vec<u8>, String> {
            let mut buf = Vec::new();
            io::stdin()
                .read_to_end(&mut buf)
                .as_ref()
                .map_err(io::Error::to_string)?;
            if format == Format::Binary {
                buf = buf[4..].to_vec();
            }
            Ok(buf)
        })?;
    Ok(match format {
        Format::Yaml => {
            serde_yaml::from_str(&String::from_utf8_lossy(&data)).map_err(|err| err.to_string())?
        }
        Format::Json => {
            serde_json::from_str(&String::from_utf8_lossy(&data)).map_err(|err| err.to_string())?
        }
        Format::Hexadecimal => T::strict_deserialize(
            Vec::<u8>::from_hex(&String::from_utf8_lossy(&data))
                .as_ref()
                .map_err(hex::Error::to_string)?,
        )?,
        Format::Binary => T::strict_deserialize(&data)?,
        _ => panic!("Can't read data from {} format", format),
    })
}

fn output_write<T>(data: T, format: Format) -> Result<(), String>
where
    T: Debug + Serialize + StrictEncode + ConsensusCommit,
    <T as ConsensusCommit>::Commitment: Display,
{
    match format {
        Format::Debug => println!("{:#?}", data),
        Format::Yaml => println!(
            "{}",
            serde_yaml::to_string(&data)
                .as_ref()
                .map_err(serde_yaml::Error::to_string)?
        ),
        Format::Json => println!(
            "{}",
            serde_json::to_string(&data)
                .as_ref()
                .map_err(serde_json::Error::to_string)?
        ),
        Format::Hexadecimal => {
            println!("{}", data.strict_serialize()?.to_hex())
        }
        Format::Rust => println!("{:#04X?}", data.strict_serialize()?),
        Format::Binary => {
            data.strict_encode(io::stdout())?;
        }
        Format::Commitment => {
            println!("{}", data.consensus_commit())
        }
        format => panic!("Can't read data in {} format", format),
    }
    Ok(())
}

fn main() -> Result<(), String> {
    let opts = Opts::parse();

    match opts.command {
        Command::Consignment { subcommand } => match subcommand {
            ConsignmentCommand::Validate { .. } => {
                /* TODO: Re-implement reading consignments from a file
                        let consignment: Consignment = input_read(consignment, input)?;
                        let schema = Schema::from_str(&schema.unwrap_or(s!(
                            "schema1qxx4qkcjsgcqehyk7gg9lrp8uqw9a34r8r0qfay0lm\
                cr3pxh7yrr2n2mvszq0s7symvkvdcf2ck6whm9zpgpqyk2nqypf8pget8vlk798ccuats4j\
                zzn98ena4p2us7eyvmxvsz5zzvcc4yu5nvjdhlw76rkxn8vvs27f0qs4qyemfdfczyvve45\
                qvfds8kryuuc4kzh03t2xruw932u6e7rn9szn8uz2kkcc7lrkzpw4ct4xpgej2s8e3vn224\
                mmwh8yjwm3c3uzcsz350urqt6gfm6wpj6gcajd6uevncqy74u87jtfmx8raza9nlm2hazyd\
                l7hyevmls6amyy4kl7rv6skggq"
                        )))?;
                        let status = consignment.validate(
                            &schema,
                            None,
                            ElectrumClient::new(&electrum).map_err(|err| format!("{:#?}", err))?,
                        );
                        println!(
                            "{}",
                            serde_yaml::to_string(&status)
                                .as_ref()
                                .map_err(serde_yaml::Error::to_string)?
                        );
                     */
            }
        },
        Command::Disclosure { subcommand } => match subcommand {
            DisclosureCommand::Convert {
                disclosure,
                input,
                output,
            } => {
                let disclosure: Disclosure = input_read(disclosure, input)?;
                output_write(disclosure, output)?;
            }
        },
        Command::Schema { subcommand } => match subcommand {
            SchemaCommand::Convert {
                schema,
                input,
                output,
            } => {
                let schema: Schema = input_read(schema, input)?;
                output_write(schema, output)?;
            }
        },
        Command::Anchor { subcommand } => match subcommand {},
        Command::Extension { subcommand } => match subcommand {
            ExtensionCommand::Convert {
                extension,
                input,
                output,
            } => {
                let extension: Extension = input_read(extension, input)?;
                output_write(extension, output)?;
            }
        },
        Command::Transition { subcommand } => match subcommand {
            TransitionCommand::Convert {
                transition,
                input,
                output,
            } => {
                let transition: Transition = input_read(transition, input)?;
                output_write(transition, output)?;
            }
        },
        Command::Genesis { subcommand } => match subcommand {
            GenesisCommand::Convert {
                genesis,
                input,
                output,
            } => {
                let genesis: Genesis = input_read(genesis, input)?;
                output_write(genesis, output)?;
            }
        },
    }

    Ok(())
}
