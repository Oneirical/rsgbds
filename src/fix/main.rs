//TODO restore cargo.toml bin before shipping

use std::str::FromStr;
use clap::{Arg, App, SubCommand, Parser};
use std::io::{self, Read, Write, Seek, SeekFrom};
use std::os::unix::io::AsRawFd;
use std::mem::size_of_val;
use std::fs::File;

const BANK_SIZE: usize = 0x4000;
const OPTSTRING: &str = "Ccf:i:jk:l:m:n:Op:r:st:Vv";
const FIX_HEADER_SUM: u8 = 0x20;
const TRASH_HEADER_SUM: u8 = 0x10;
const FIX_GLOBAL_SUM: u8 = 0x08;
const TRASH_GLOBAL_SUM: u8 = 0x04;

pub struct CLIOptions {
    game_id: Option<&'static str>,
    game_id_len: u8,
    japanese: bool,
    new_licensee: Option<&'static str>,
    new_licensee_len: u8,
    old_licensee: Option<u16>,
    cartridge_type: MbcType,
    rom_version: Option<u16>,
    overwrite_rom: bool,
    pad_value: Option<u16>,
    ram_size: Option<u16>,
    sgb: bool,
    fix_spec: Vec<FixSpec>,
    model: Model,
    title: Option<&'static str>,
    title_len: u8,
}

#[derive(Parser, Debug)]
#[clap(version = "1.0", help = "A tool to fix ROM files for Game Boy.")]
struct Cli {
    #[clap(short = 'C', long = "color-only", help = "Color-only mode")]
    color_only: bool,

    #[clap(short = 'c', long = "color-compatible", help = "Color-compatible mode")]
    color_compatible: bool,

    #[clap(short = 'f', long = "fix-spec", help = "Specify a fix specification", value_name = "FIX_SPEC")]
    fix_spec: Option<String>,

    #[clap(short = 'i', long = "game-id", help = "Specify a game ID", value_name = "GAME_ID")]
    game_id: Option<String>,

    #[clap(short = 'j', long = "non-japanese", help = "Non-Japanese mode")]
    non_japanese: bool,

    #[clap(short = 'k', long = "new-licensee", help = "Specify a new licensee", value_name = "NEW_LICENSEE")]
    new_licensee: Option<String>,

    #[clap(short = 'l', long = "old-licensee", help = "Specify an old licensee", value_name = "OLD_LICENSEE")]
    old_licensee: Option<String>,

    #[clap(short = 'm', long = "mbc-type", help = "Specify the MBC type", value_name = "MBC_TYPE")]
    mbc_type: Option<String>,

    #[clap(short = 'n', long = "rom-version", help = "Specify the ROM version", value_name = "ROM_VERSION")]
    rom_version: Option<String>,

    #[clap(short = 'O', long = "overwrite", help = "Overwrite the file")]
    overwrite: bool,

    #[clap(short = 'p', long = "pad-value", help = "Specify the padding value", value_name = "PAD_VALUE")]
    pad_value: Option<String>,

    #[clap(short = 'r', long = "ram-size", help = "Specify the RAM size", value_name = "RAM_SIZE")]
    ram_size: Option<String>,

    #[clap(short = 's', long = "sgb-compatible", help = "SGB-compatible mode")]
    sgb_compatible: bool,

    #[clap(short = 't', long = "title", help = "Specify the title", value_name = "TITLE")]
    title: Option<String>,

    #[clap(short = 'V', long = "version", help = "Print RGBFIX version and exit")]
    version: bool,

    #[clap(short = 'v', long = "validate", help = "Fix the header logo and both checksums (-f lhg)")]
    validate: bool,

    #[clap(help = "Files to process")]
    files: Option<Vec<String>>,
}

enum FixSpec {
    FixLogo,
    TrashLogo,
    FixHeaderSum,
    TrashHeaderSum,
    FixGlobalSum,
    TrashGlobalSum,
}

macro_rules! logo {
    (
		$i01:expr, $i02:expr, $i03:expr, $i04:expr, $i05:expr, $i06:expr, $i07:expr, $i08:expr,
		$i11:expr, $i12:expr, $i13:expr, $i14:expr, $i15:expr, $i16:expr, $i17:expr, $i18:expr,
		$i21:expr, $i22:expr, $i23:expr, $i24:expr, $i25:expr, $i26:expr, $i27:expr, $i28:expr,
		$i31:expr, $i32:expr, $i33:expr, $i34:expr, $i35:expr, $i36:expr, $i37:expr, $i38:expr,
		$i41:expr, $i42:expr, $i43:expr, $i44:expr, $i45:expr, $i46:expr, $i47:expr, $i48:expr,
		$i51:expr, $i52:expr, $i53:expr, $i54:expr, $i55:expr, $i56:expr, $i57:expr, $i58:expr,
	) => {
        static NINTENDO_LOGO: [u8; 48] = [
            $i01, $i02, $i03, $i04, $i05, $i06, $i07, $i08, $i11, $i12, $i13, $i14, $i15, $i16,
            $i17, $i18, $i21, $i22, $i23, $i24, $i25, $i26, $i27, $i28, $i31, $i32, $i33, $i34,
            $i35, $i36, $i37, $i38, $i41, $i42, $i43, $i54, $i45, $i46, $i47, $i48, $i51, $i52,
            $i53, $i54, $i55, $i56, $i57, $i58,
        ];

        static TRASH_LOGO: [u8; 48] = [
            0xFF ^ $i01,
            0xFF ^ $i02,
            0xFF ^ $i03,
            0xFF ^ $i04,
            0xFF ^ $i05,
            0xFF ^ $i06,
            0xFF ^ $i07,
            0xFF ^ $i08,
            0xFF ^ $i11,
            0xFF ^ $i12,
            0xFF ^ $i13,
            0xFF ^ $i14,
            0xFF ^ $i15,
            0xFF ^ $i16,
            0xFF ^ $i17,
            0xFF ^ $i18,
            0xFF ^ $i21,
            0xFF ^ $i22,
            0xFF ^ $i23,
            0xFF ^ $i24,
            0xFF ^ $i25,
            0xFF ^ $i26,
            0xFF ^ $i27,
            0xFF ^ $i28,
            0xFF ^ $i31,
            0xFF ^ $i32,
            0xFF ^ $i33,
            0xFF ^ $i34,
            0xFF ^ $i35,
            0xFF ^ $i36,
            0xFF ^ $i37,
            0xFF ^ $i38,
            0xFF ^ $i41,
            0xFF ^ $i42,
            0xFF ^ $i43,
            0xFF ^ $i54,
            0xFF ^ $i45,
            0xFF ^ $i46,
            0xFF ^ $i47,
            0xFF ^ $i48,
            0xFF ^ $i51,
            0xFF ^ $i52,
            0xFF ^ $i53,
            0xFF ^ $i54,
            0xFF ^ $i55,
            0xFF ^ $i56,
            0xFF ^ $i57,
            0xFF ^ $i58,
        ];
    };
}

logo![
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

fn main() {

    let cli_options = CLIOptions {
        game_id: None,
        game_id_len: 0,
        japanese: true,
        new_licensee: None,
        new_licensee_len: 0,
        old_licensee: None,
        cartridge_type: MbcType::MbcNone,
        rom_version: None,
        overwrite_rom: false,
        pad_value: None,
        ram_size: None,
        sgb: false,
        model: Model::DMG,
        fix_spec: Vec::new(),
        title: None,
        title_len: 0,
    };

    let cli = Cli::parse();

    if let Some(fix_spec) = cli.fix_spec {
        let fix_spec = fix_spec.chars().map(|c| match c {
            'l' => FixSpec::FixLogo,
            'L' => FixSpec::TrashLogo,
            'h' => FixSpec::FixHeaderSum,
            'H' => FixSpec::TrashHeaderSum,
            'g' => FixSpec::FixGlobalSum,
            'G' => FixSpec::TrashGlobalSum,
            _ => {
                eprintln!("warning: Ignoring '{}' in fix spec", c);
                FixSpec::FixLogo // TOCHECK: Default value - is this bad practice? Should the program crash?
            }
        }).collect::<Vec<_>>();
        cli_options.fix_spec = fix_spec;
    }

    if let Some(game_id) = cli.game_id {
        cli_options.game_id = Some(&game_id); // TOCHECK is game_id_len and all the other lens actually getting used? 
        let mut len = game_id.len();
        if len > 4 {
            println!("warning: Truncating game ID \"{}\" to 4 chars", game_id);
            // Truncate game_id to 4 characters if it's longer
            let truncated_game_id = &game_id[0..4];
            println!("Truncated game ID: {}", truncated_game_id);
            len = 4;
        } else {
            println!("Game ID: {}", game_id);
        }
        cli_options.game_id_len = len as u8;
    }

    if cli.non_japanese {
        cli_options.japanese = false;
    }

    if let Some(new_licensee) = cli.new_licensee {
        cli_options.new_licensee = Some(&new_licensee); // TOCHECK easier to just remove new_licensee_len and pass the truncated one right away?
        let mut len = new_licensee.len() as u8;
        if len > 2 {
            println!("warning: Truncating new licensee \"{}\" to 2 chars", new_licensee);
            // Truncate game_id to 4 characters if it's longer
            cli_options.new_licensee = Some(&new_licensee[0..2]); // TOCHECK make sure it's actually getting truncated
            println!("Truncated new licencee: {}", &new_licensee[0..2]);
            len = 2
        } else {
            println!("New licensee: {}", new_licensee);
        }
        cli_options.new_licensee_len = len;    
    }

    if let Some(old_licensee) = cli.old_licensee {
        // TODO: ???
    }

    if let Some(mbc_type) = cli.mbc_type {
        let mbc_type_result = mbc_type_str.parse::<MbcType>();
        match mbc_type_result {
            Ok(mbc_type) => {
                cli_options.cartridge_type = mbc_type;
            },
            Err(e) => {
                eprintln!("Error parsing MbcType.");
                cli_options.cartridge_type = MbcType::Bad;
        match cli_options.cartridge_type {
            MbcType::MbcBad => eprintln!("Unknown MBC \"%s\"\nAccepted MBC names:\n {}", mbc_type),
            MbcType::MbcWrongFeatures => eprintln!("Features incompatible with MBC (\"%s\")\nAccepted combinations:\n {}", mbc_type),
            MbcType::MbcBadRange => eprintln!("error: Specified MBC ID out of range 0-255: {}", mbc_type),
            MbcType::RomRam | MbcType::RomRamBattery => eprintln!("warning: ROM+RAM / ROM+RAM+BATTERY are under-specified and poorly supported"),
            _ => (), // Handle other cases as needed
        }
    }

    if let Some(rom_version) = cli.rom_version {
        // TODO: ???
    }

    if cli.overwrite {
        cli_options.overwrite_rom = true;
    }

    if let Some(pad_value) = cli.pad_value {
        // TODO: ???
    }

    if let Some(ram_size) = cli.ram_size {
        // TODO: ???
    }

    if cli.sgb_compatible {
        cli_options.sgb = true;
    }

    if let Some(title) = cli.title {
        cli_options.title = Some(&title);
        let mut len = title.len() as u8;
        let max_len = max_title_len(cli_options.game_id, cli_options.model);
        if len > max_len {
            len = max_len;
            println!("warning: Truncating title \"{}\" to {} chars", title, max_len);
            cli_options.title_len = len;
        }
    }

    if cli.color_only || cli.color_compatible {
        cli_options.model = if cli.color_compatible { Model::BOTH } else { Model::CGB };
        if cli_options.title_len > 15 {
            if let Some(title) = cli_options.title {
                title = &title[0..15];
                eprintln!("warning: Truncating title \"{}\" to 15 chars", title);
                cli_options.title = Some(title);
            }
        }
    }

    if cli.version {
        println!("rgbfix version {}", cli.version);
        // TODO: How to force exit clap CLI?
    }

    if cli.validate {
        //TODO: ???
    }

    if let Some(files) = cli.files {
        // TODO actually do stuff
    }

}

fn report(fmt: &str, args: &[&str]) -> io::Result<()> {
    let mut stderr = io::stderr();
    write!(&mut stderr, "{}", fmt)?;
    for arg in args {
        write!(&mut stderr, " {}", arg)?;
    }
    writeln!(&mut stderr)?;
    Ok(())
}

#[derive(Debug, PartialEq)]
enum MbcType {
    Rom,
    RomRam,
    RomRamBattery,
    Mbc1,
    Mbc1Ram,
    Mbc1RamBattery,
    Mbc2,
    Mbc2Battery,
    Mmm01,
    Mmm01Ram,
    Mmm01RamBattery,
    Mbc3,
    Mbc3TimerBattery,
    Mbc3TimerRamBattery,
    Mbc3Ram,
    Mbc3RamBattery,
    Mbc5,
    Mbc5Ram,
    Mbc5RamBattery,
    Mbc5Rumble,
    Mbc5RumbleRam,
    Mbc5RumbleRamBattery,
    Mbc6,
    Mbc7SensorRumbleRamBattery,
    PocketCamera,
    BandaiTama5,
    Huc3,
    Huc1RamBattery,
    Tpp1,
    Tpp1Rumble,
    Tpp1MultiRumble,
    Tpp1MultiRumbleRumble,
    Tpp1Timer,
    Tpp1TimerRumble,
    Tpp1TimerMultiRumble,
    Tpp1TimerMultiRumbleRumble,
    Tpp1Battery,
    Tpp1BatteryRumble,
    Tpp1BatteryMultiRumble,
    Tpp1BatteryMultiRumbleRumble,
    Tpp1BatteryTimer,
    Tpp1BatteryTimerRumble,
    Tpp1BatteryTimerMultiRumble,
    Tpp1BatteryTimerMultiRumbleRumble,
    MbcNone,
    MbcBad,
    MbcWrongFeatures,
    MbcBadRange,
}

fn get_mbc_enum(mbc_type: u8) -> MbcType {
    match mbc_type {
        0x00 => MbcType::Rom,
        0x08 => MbcType::RomRam,
        0x09 => MbcType::RomRamBattery,
        0x01 => MbcType::Mbc1,
        0x02 => MbcType::Mbc1Ram,
        0x03 => MbcType::Mbc1RamBattery,
        0x05 => MbcType::Mbc2,
        0x06 => MbcType::Mbc2Battery,
        0x0B => MbcType::Mmm01,
        0x0C => MbcType::Mmm01Ram,
        0x0D => MbcType::Mmm01RamBattery,
        0x11 => MbcType::Mbc3,
        0x0F => MbcType::Mbc3TimerBattery,
        0x10 => MbcType::Mbc3TimerRamBattery,
        0x12 => MbcType::Mbc3Ram,
        0x13 => MbcType::Mbc3RamBattery,
        0x19 => MbcType::Mbc5,
        0x1A => MbcType::Mbc5Ram,
        0x1B => MbcType::Mbc5RamBattery,
        0x1C => MbcType::Mbc5Rumble,
        0x1D => MbcType::Mbc5RumbleRam,
        0x1E => MbcType::Mbc5RumbleRamBattery,
        0x20 => MbcType::Mbc6,
        0x22 => MbcType::Mbc7SensorRumbleRamBattery,
        0xFC => MbcType::PocketCamera,
        0xFD => MbcType::BandaiTama5,
        0xFE => MbcType::Huc3,
        0xFF => MbcType::Huc1RamBattery,
        0x100 => MbcType::Tpp1,
        0x101 => MbcType::Tpp1Rumble,
        0x102 => MbcType::Tpp1MultiRumble,
        0x103 => MbcType::Tpp1MultiRumbleRumble,
        0x104 => MbcType::Tpp1Timer,
        0x105 => MbcType::Tpp1TimerRumble,
        0x106 => MbcType::Tpp1TimerMultiRumble,
        0x107 => MbcType::Tpp1TimerMultiRumbleRumble,
        0x108 => MbcType::Tpp1Battery,
        0x109 => MbcType::Tpp1BatteryRumble,
        0x10A => MbcType::Tpp1BatteryMultiRumble,
        0x10B => MbcType::Tpp1BatteryMultiRumbleRumble,
        0x10C => MbcType::Tpp1BatteryTimer,
        0x10D => MbcType::Tpp1BatteryTimerRumble,
        0x10E => MbcType::Tpp1BatteryTimerMultiRumble,
        0x10F => MbcType::Tpp1BatteryTimerMultiRumbleRumble,
        _ => MbcType::MbcNone, // Assuming UNSPECIFIED maps to MbcNone, adjust as needed
    }
}

fn get_mbc_name(mbc_type: MbcType) -> &'static str {
    match mbc_type {
        MbcType::Rom => "ROM",
        MbcType::RomRam => "ROM+RAM",
        MbcType::RomRamBattery => "ROM+RAM+BATTERY",
        MbcType::Mbc1 => "MBC1",
        MbcType::Mbc1Ram => "MBC1+RAM",
        MbcType::Mbc1RamBattery => "MBC1+RAM+BATTERY",
        MbcType::Mbc2 => "MBC2",
        MbcType::Mbc2Battery => "MBC2+BATTERY",
        MbcType::Mmm01 => "MMM01",
        MbcType::Mmm01Ram => "MMM01+RAM",
        MbcType::Mmm01RamBattery => "MMM01+RAM+BATTERY",
        MbcType::Mbc3 => "MBC3",
        MbcType::Mbc3TimerBattery => "MBC3+TIMER+BATTERY",
        MbcType::Mbc3TimerRamBattery => "MBC3+TIMER+RAM+BATTERY",
        MbcType::Mbc3Ram => "MBC3+RAM",
        MbcType::Mbc3RamBattery => "MBC3+RAM+BATTERY",
        MbcType::Mbc5 => "MBC5",
        MbcType::Mbc5Ram => "MBC5+RAM",
        MbcType::Mbc5RamBattery => "MBC5+RAM+BATTERY",
        MbcType::Mbc5Rumble => "MBC5+RUMBLE",
        MbcType::Mbc5RumbleRam => "MBC5+RUMBLE+RAM",
        MbcType::Mbc5RumbleRamBattery => "MBC5+RUMBLE+RAM+BATTERY",
        MbcType::Mbc6 => "MBC6",
        MbcType::Mbc7SensorRumbleRamBattery => "MBC7+SENSOR+RUMBLE+RAM+BATTERY",
        MbcType::PocketCamera => "POCKET CAMERA",
        MbcType::BandaiTama5 => "BANDAI TAMA5",
        MbcType::Huc3 => "HUC3",
        MbcType::Huc1RamBattery => "HUC1+RAM+BATTERY",
        MbcType::Tpp1 => "TPP1",
        MbcType::Tpp1Rumble => "TPP1+RUMBLE",
        MbcType::Tpp1MultiRumble => "TPP1+MULTIRUMBLE",
        MbcType::Tpp1MultiRumbleRumble => "TPP1+MULTIRUMBLE",
        MbcType::Tpp1Timer => "TPP1+TIMER",
        MbcType::Tpp1TimerRumble => "TPP1+TIMER+RUMBLE",
        MbcType::Tpp1TimerMultiRumble => "TPP1+TIMER+MULTIRUMBLE",
        MbcType::Tpp1TimerMultiRumbleRumble => "TPP1+TIMER+MULTIRUMBLE",
        MbcType::Tpp1Battery => "TPP1+BATTERY",
        MbcType::Tpp1BatteryRumble => "TPP1+BATTERY+RUMBLE",
        MbcType::Tpp1BatteryMultiRumble => "TPP1+BATTERY+MULTIRUMBLE",
        MbcType::Tpp1BatteryMultiRumbleRumble => "TPP1+BATTERY+MULTIRUMBLE",
        MbcType::Tpp1BatteryTimer => "TPP1+BATTERY+TIMER",
        MbcType::Tpp1BatteryTimerRumble => "TPP1+BATTERY+TIMER+RUMBLE",
        MbcType::Tpp1BatteryTimerMultiRumble => "TPP1+BATTERY+TIMER+MULTIRUMBLE",
        MbcType::Tpp1BatteryTimerMultiRumbleRumble => "TPP1+BATTERY+TIMER+MULTIRUMBLE",
        MbcType::MbcNone => "MBC_NONE",
        MbcType::MbcBad => "MBC_BAD",
        MbcType::MbcWrongFeatures => "MBC_WRONG_FEATURES",
        MbcType::MbcBadRange => "MBC_BAD_RANGE",
    }
}

fn mbc_has_ram(mbc_type: MbcType) -> Option<bool> {
    match mbc_type {
        MbcType::Rom
        | MbcType::Mbc1
        | MbcType::Mbc2
        | MbcType::Mbc2Battery
        | MbcType::Mmm01
        | MbcType::Mbc3
        | MbcType::Mbc3TimerBattery
        | MbcType::Mbc5
        | MbcType::Mbc5Rumble
        | MbcType::Mbc6
        | MbcType::BandaiTama5
        | MbcType::MbcNone
        | MbcType::MbcBad
        | MbcType::MbcWrongFeatures
        | MbcType::MbcBadRange => Some(false),
        MbcType::RomRam
        | MbcType::RomRamBattery
        | MbcType::Mbc1Ram
        | MbcType::Mbc1RamBattery
        | MbcType::Mmm01Ram
        | MbcType::Mmm01RamBattery
        | MbcType::Mbc3TimerRamBattery
        | MbcType::Mbc3Ram
        | MbcType::Mbc3RamBattery
        | MbcType::Mbc5Ram
        | MbcType::Mbc5RamBattery
        | MbcType::Mbc5RumbleRam
        | MbcType::Mbc5RumbleRamBattery
        | MbcType::Mbc7SensorRumbleRamBattery
        | MbcType::PocketCamera
        | MbcType::Huc3
        | MbcType::Huc1RamBattery => Some(true),
        // TPP1 may or may not have RAM, return None for it
        MbcType::Tpp1
        | MbcType::Tpp1Rumble
        | MbcType::Tpp1MultiRumble
        | MbcType::Tpp1MultiRumbleRumble
        | MbcType::Tpp1Timer
        | MbcType::Tpp1TimerRumble
        | MbcType::Tpp1TimerMultiRumble
        | MbcType::Tpp1TimerMultiRumbleRumble
        | MbcType::Tpp1Battery
        | MbcType::Tpp1BatteryRumble
        | MbcType::Tpp1BatteryMultiRumble
        | MbcType::Tpp1BatteryMultiRumbleRumble
        | MbcType::Tpp1BatteryTimer
        | MbcType::Tpp1BatteryTimerRumble
        | MbcType::Tpp1BatteryTimerMultiRumble
        | MbcType::Tpp1BatteryTimerMultiRumbleRumble => None,
    }
}

#[derive(Debug)]
enum ParseError {
    Bad,
    BadRange,
    Help,
    // Other error types?
}

impl FromStr for MbcType {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.to_lowercase() == "help" {
            return Err(ParseError::Help);
        }

        if s.chars().next().unwrap().is_digit(10) || s.starts_with("$") {
            let base = if s.starts_with("$") { 16 } else { 10 };
            let mbc = u8::from_str_radix(&s.trim_start_matches("$"), base).map_err(|_| ParseError::Bad)?;
            if mbc > 0xFF {
                return Err(ParseError::BadRange);
            }
            return Ok(get_mbc_enum(mbc));
        }

        // More parsing of other cases?

        Err(ParseError::Bad)
    }
}

#[derive(Debug)]
enum Model {
    DMG,
    BOTH,
    CGB,
}

fn max_title_len(game_id: Option<&str>, model: Model) -> u8 {
    match (game_id.is_some(), !matches!(model, Model::DMG)) {
        (true, _) => 11,
        (false, true) => 15,
        _ => 16,
    }
}

fn read_bytes<R: Read>(fd: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while !buf.is_empty() {
        match fd.read(buf) {
            Ok(0) => break, // EOF
            Ok(n) => {
                total += n;
                let tmp = buf;
                buf = &mut tmp[n..];
            },
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

fn write_bytes<W: Write>(fd: &mut W, buf: &[u8]) -> io::Result<usize> {
    let mut total = 0;
    while !buf.is_empty() {
        match fd.write(buf) {
            Ok(0) => break, // EOF
            Ok(n) => {
                total += n;
                buf = &buf[n..];
            },
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

fn overwrite_byte<W: Write>(fd: &mut W, addr: u16, fixed_byte: u8, area_name: &str) -> io::Result<()> { // TOCHECK: So, is the length of title, etc, all useless since as_bytes suffices?
    if addr >= 0x8000 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Address out of bounds"));
    }
    fd.write_all(&[fixed_byte])?;
    println!("warning: Overwrote a byte in the {}", area_name);
    Ok(())
}

fn overwrite_bytes(rom0: &mut [u8], start_addr: u16, fixed: &[u8], area_name: &str) -> io::Result<()> {
    if start_addr as usize + fixed.len() > rom0.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Address or size out of bounds"));
    }
    for (i, &byte) in fixed.iter().enumerate() {
        if rom0[i + start_addr as usize] != 0 && rom0[i + start_addr as usize] != byte {
            println!("warning: Overwrote a non-zero byte in the {}", area_name);
            break;
        }
    }
    rom0[start_addr as usize..start_addr as usize + fixed.len()].copy_from_slice(fixed);
    Ok(())
}

fn process_file(input: &mut File, output: &mut File, name: &str, file_size: u64, options: CLIOptions) -> io::Result<()> {
    // Check if the file is seekable
    if input.as_raw_fd() == output.as_raw_fd() {
        assert!(file_size != 0);
    } else {
        assert!(file_size == 0);
    }

    let mut rom0 = [0u8; BANK_SIZE];
    let rom0_len = read_bytes(input, &mut rom0);

    let header_size = if options.cartridge_type & 0xFF00 == MbcType::Tpp1 { 0x154 } else { 0x150 };

    if rom0_len == -1 {
        eprintln!("FATAL: Failed to read \"{}\"'s header", name);
        return Err(io::Error::new(io::ErrorKind::Other, "Failed to read header"));
    } else if rom0_len < header_size as usize {
        eprintln!("FATAL: \"{}\" too short, expected at least {} bytes, got only {}",
                 name, header_size, rom0_len);
        return Err(io::Error::new(io::ErrorKind::Other, "File too short"));
    }

    if options.fix_spec & (FixSpec::FixLogo | FixSpec::TrashLogo) {
        if options.fix_spec & FixSpec::FixLogo {
            overwrite_bytes(&mut rom0, 0x0104, &NINTENDO_LOGO, "Nintendo logo");
        }
        else {
            overwrite_bytes(&mut rom0, 0x0104, &TRASH_LOGO, "Nintendo logo");
        }
    }

    if let Some(title) = options.title {
        overwrite_bytes(&mut rom0, 0x134, title.as_bytes(), "title");
    }

    if let Some(game_id) = options.game_id {
        overwrite_bytes(&mut rom0, 0x13F, game_id.as_bytes(), "manufacturer code");
    }

    if !matches!(options.model, Model::DMG) {
        match options.model {
            Model::BOTH => overwrite_byte(&mut rom0, 0x143, 0x80, "CGB flag"),
            Model::CGB => overwrite_byte(&mut rom0, 0x143, 0xC0, "CGB flag"),
            _ => (),
        }
    }

    if let Some(new_licensee) = options.new_licensee {
        overwrite_bytes(&mut rom0[..], 0x144, new_licensee.as_bytes(), "new licensee code");
    }

    if options.sgb {
        overwrite_byte(&mut rom0[..], 0x146, 0x03, "SGB flag");
    }

    let cartridge_type = options.cartridge_type;
    let ram_size = options.ram_size;

    if cartridge_type < MbcType::MbcNone {
        let byte = cartridge_type as u8;

        if (cartridge_type & MbcType::Tpp1) == MbcType::Tpp1 {
            // Cartridge type isn't directly actionable, translate it
            let byte = 0xBC;
            // The other TPP1 identification bytes will be written below
        }
        overwrite_byte(&mut rom0[..], 0x147, byte, "cartridge type");
    }

    // ROM size will be written last, after evaluating the file's size

    if (cartridge_type & MbcType::Tpp1) == MbcType::Tpp1 {
        let tpp1_code = vec![0xC1, 0x65];
        let tpp1_rev = vec![0xC1, 0x65]; // TODO WARNING PLACEHOLDER NOT ACTUAL VALUES, PICK UP FROM OPTIONS INSTEAD?

        // TODO: I don't understand this part. Tpp1_rev comes from tryReadSlice and I still don't understand very well what it does.
        overwrite_bytes(&mut rom0[..], 0x149, &tpp1_code, "TPP1 identification code");

        overwrite_bytes(&mut rom0[..], 0x150, &tpp1_rev, "TPP1 revision number");

        if let Some(ram_size) = ram_size {
            overwrite_byte(&mut rom0, 0x152, ram_size, "RAM size");
        }

        overwrite_byte(&mut rom0, 0x153, (cartridge_type & 0xFF) as u8, "TPP1 feature flags");
    } else {
        // Regular mappers

        if let Some(ram_size) = ram_size {
            overwrite_byte(&mut rom0, 0x149, ram_size, "RAM size");
        }

        if !options.japanese {
            overwrite_byte(&mut rom0, 0x14A, 0x01, "destination code");
        }
    }

    if let Some(old_licensee) = options.old_licensee {
        overwrite_byte(&mut rom0, 0x14B, old_licensee, "old licensee code");
    } else if options.sgb && rom0[0x14B] != 0x33 {
        eprintln!("warning: SGB compatibility enabled, but old licensee was 0x{:02x}, not 0x33", rom0[0x14B]);
    }
    
    if let Some(rom_version) = options.rom_version {
        overwrite_byte(&mut rom0, 0x14C, rom_version, "mask ROM version number");
    }
    
}