use std::collections::HashMap;

use rgbds::{
    rpn::Rpn,
    section::{Kind, Modifier},
    RelocKind, TruncationLevel,
};
use string_interner::{backend::StringBackend, symbol::SymbolU32, StringInterner};

use crate::{
    expr::{ByteOrExpr, Expression},
    fstack::Fstack,
    input::SourceString,
    language::{AsmError, AsmErrorKind, Location, Warning},
    macro_args::MacroArgs,
    symbols::Symbols,
};

#[derive(Debug)]
pub struct Sections<'fstack> {
    names: StringInterner<StringBackend<SymbolU32>>,
    sections: HashMap<SymbolU32, SectionData<'fstack>>,
    stack: Vec<Option<ActiveSection>>,
}

#[derive(Debug, Clone)]
pub struct SectionId(SymbolU32);

impl<'fstack> Sections<'fstack> {
    pub fn new() -> Self {
        let mut stack = Vec::with_capacity(2); // I have never seen nested `PUSHS`.
        stack.push(None);

        Self {
            names: StringInterner::new(),
            sections: HashMap::new(),
            stack,
        }
    }

    pub fn add_section(
        &mut self,
        name_string: SourceString,
        kind: Kind,
        modifier: Modifier,
        address: Option<Expression<'fstack>>,
        attrs: SectionAttributes<'fstack>,
        def_begin: Location<'fstack>,
        def_end: Location<'fstack>,
        symbols: &Symbols,
        macro_args: Option<&MacroArgs>,
    ) -> Result<(), AsmError<'fstack>> {
        let name = self.names.get_or_intern(&name_string);
        // `try_insert` would be nicer, but it's unstable for now.
        if let Some(other) = self.sections.get(&name) {
            // TODO: merging

            let other_def_info =
                Fstack::make_diag_info(&other.definition.0, Some(&other.definition.1));
            return Err(AsmError {
                begin: def_begin,
                end: def_end,
                kind: AsmErrorKind::SectAlreadyDefined(name_string, other_def_info),
            });
        }

        let (attrs, def_begin, def_end) = NormalizedSectAttrs::try_new(
            kind, address, attrs, def_begin, def_end, symbols, macro_args, self,
        )?;
        self.sections.insert(
            name,
            SectionData::new(kind, modifier, (def_begin, def_end), attrs),
        );

        // Make the section "active".
        *self.stack.last_mut().unwrap() = Some(ActiveSection::new(name));

        Ok(())
    }

    pub fn active_section<'a>(&'a self) -> Option<SectionHandle<'a, 'fstack>> {
        let top_slot = self.stack.last().and_then(|slot| slot.as_ref())?;
        Some(SectionHandle(
            top_slot,
            self.sections.get(&top_slot.name).unwrap(),
        ))
    }

    pub fn active_section_mut<'a>(&'a mut self) -> Option<SectionHandleMut<'a, 'fstack>> {
        let top_slot = self.stack.last_mut().and_then(|slot| slot.as_mut())?;
        Some(SectionHandleMut(
            top_slot,
            self.sections.get_mut(&top_slot.name).unwrap(),
        ))
    }
}

#[derive(Debug)]
pub struct SectionData<'fstack> {
    kind: Kind,
    modifier: Modifier,
    definition: (Location<'fstack>, Location<'fstack>),
    address: Option<u16>,
    bank: Option<u32>,
    alignment: u8,
    align_offset: u16,

    patches: Vec<Relocation<'fstack>>,
    /// This vector is only used if `self.kind.has_data()`.
    data: Vec<u8>,
    /// This is not redundant with `self.data.len()`, as the former is capped by the kind's size.
    len_virt: usize,
}

impl<'fstack> SectionData<'fstack> {
    fn new(
        kind: Kind,
        modifier: Modifier,
        definition: (Location<'fstack>, Location<'fstack>),
        NormalizedSectAttrs {
            address,
            bank,
            alignment,
            align_offset,
        }: NormalizedSectAttrs,
    ) -> Self {
        Self {
            kind,
            modifier,
            definition,
            address,
            bank,
            alignment,
            align_offset,

            patches: vec![],
            data: vec![],
            len_virt: 0,
        }
    }
}

#[derive(Debug)]
struct Relocation<'fstack> {
    definition: (Location<'fstack>, Location<'fstack>),
    /// Offset into the parent section's data where the patch must be applied.
    offset: usize,
    /// Which section PC belongs to; not necessarily the same as the parent section due to `LOAD`.
    pc_section: Option<SymbolU32>,
    /// Offset of PC into the "PC section".
    pc_offset: usize,
    kind: RelocKind,
    rpn: Rpn,
}

#[derive(Debug)]
struct ActiveSection {
    name: SymbolU32,

    offset: usize,
    pc_section: Option<SymbolU32>,
    pc_offset: usize,
    label_scope: Option<SymbolU32>,
    union_stack: Vec<Union>,
}

impl ActiveSection {
    fn new(name: SymbolU32) -> Self {
        Self {
            name,

            offset: 0,
            pc_section: None,
            pc_offset: 0,
            label_scope: None,
            union_stack: vec![],
        }
    }
}

pub struct SectionHandle<'a, 'fstack>(&'a ActiveSection, &'a SectionData<'fstack>);

impl<'fstack> SectionHandle<'_, 'fstack> {
    pub fn try_get_pc(&self) -> Option<u16> {
        self.1.address.map(|base_addr| {
            base_addr.wrapping_add(self.1.data.len().try_into().unwrap_or(u16::MAX))
        })
    }
}

pub struct SectionHandleMut<'a, 'fstack>(&'a mut ActiveSection, &'a mut SectionData<'fstack>);

impl<'fstack> SectionHandleMut<'_, 'fstack> {
    pub fn extend<
        S: IntoIterator<Item = ByteOrExpr<'fstack>> + AsRef<[ByteOrExpr<'fstack>]>,
        F: FnMut(Warning),
    >(
        &mut self,
        slice: S,
        mut warn: F,
    ) -> Result<(), AsmErrorKind> {
        if !self.1.kind.has_data() {
            return Err(AsmErrorKind::NotCodeSection(self.1.kind));
        }

        let total_len = slice.as_ref().iter().fold(0, |len, item| {
            len + match item {
                ByteOrExpr::Byte(_) => 1,
                ByteOrExpr::Expr(_, _, _, expr_kind) => expr_kind.width(),
            }
        });
        self.1.len_virt = self.1.len_virt.saturating_add(total_len.into());

        if self.1.len_virt <= self.1.kind.size(true, true).into() {
            for item in slice.into_iter() {
                let len = match item {
                    ByteOrExpr::Byte(byte) => {
                        self.1.data.push(byte);
                        1
                    }
                    ByteOrExpr::Expr(begin, end, rpn, kind) => {
                        let len = kind.width();
                        let data = match rpn.try_get_constant() {
                            Some(constant) => {
                                if let Some(level) = match kind.is_in_range(constant) {
                                    TruncationLevel::None => None,
                                    TruncationLevel::Loose => Some(2),
                                    TruncationLevel::Strict => Some(1),
                                } {
                                    warn(Warning {
                                        begin,
                                        end,
                                        kind: crate::language::WarningKind::Truncation {
                                            level,
                                            width: kind.width() * 8,
                                        },
                                    });
                                }

                                constant.to_le_bytes()
                            }
                            None => {
                                self.1.patches.push(Relocation {
                                    definition: (begin, end),
                                    offset: self.0.offset,
                                    pc_section: self.0.pc_section,
                                    pc_offset: self.0.pc_offset,
                                    kind,
                                    rpn,
                                });
                                [0; 4] // Write some dummy bytes that will be overwritten during linking.
                            }
                        };
                        self.1.data.extend_from_slice(&data[..len.into()]);

                        len
                    }
                };

                // Advance the offset.
                self.0.offset += usize::from(len);
                self.0.pc_offset += usize::from(len);
            }
            debug_assert_eq!(self.1.len_virt, self.1.data.len());
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Union {
    start_ofs: usize,
    len: usize,
}

#[derive(Debug, Default)]
pub struct SectionAttributes<'fstack> {
    pub(crate) bank: Option<Expression<'fstack>>,
    pub(crate) alignment: Option<Expression<'fstack>>,
    pub(crate) offset: Option<Expression<'fstack>>,
}

#[derive(Debug)]
struct NormalizedSectAttrs {
    address: Option<u16>,
    bank: Option<u32>,
    alignment: u8,
    align_offset: u16,
}

impl NormalizedSectAttrs {
    fn try_new<'fstack>(
        kind: Kind,
        address: Option<Expression<'fstack>>,
        attrs: SectionAttributes<'fstack>,
        def_begin: Location<'fstack>,
        def_end: Location<'fstack>,
        symbols: &Symbols,
        macro_args: Option<&MacroArgs>,
        sections: &Sections,
    ) -> Result<(Self, Location<'fstack>, Location<'fstack>), AsmError<'fstack>> {
        let banks = kind.banks(true);
        let start_addr = kind.start_addr();

        // First, "lower" the raw expressions into something easier to manipulate.

        fn eval<'fstack, T, F: FnOnce(i32) -> Result<T, AsmErrorKind>>(
            opt: Option<Expression<'fstack>>,
            constrain: F,
            symbols: &Symbols,
            macro_args: Option<&MacroArgs>,
            sections: &Sections,
        ) -> Result<Option<T>, AsmError<'fstack>> {
            opt.map(|expr| {
                expr.try_eval(symbols, macro_args, sections)
                    .and_then(|(value, begin, end)| {
                        constrain(value).map_err(|kind| AsmError { begin, end, kind })
                    })
            })
            .transpose()
        }
        let mut address = eval(
            address,
            |addr| {
                addr.try_into()
                    .map_err(|_| AsmErrorKind::AddrOutOfRange(addr))
                    .and_then(|addr: u16| {
                        let size = kind.size(true, true);
                        if addr.wrapping_sub(start_addr) < size {
                            Ok(addr)
                        } else {
                            Err(AsmErrorKind::AddrOutOfBounds(
                                addr,
                                start_addr,
                                start_addr + (size - 1),
                            ))
                        }
                    })
            },
            symbols,
            macro_args,
            sections,
        )?;
        let mut bank = eval(
            attrs.bank,
            |bank| {
                if !matches!(kind, Kind::Romx | Kind::Vram | Kind::Sram | Kind::Wramx) {
                    return Err(AsmErrorKind::Unbanked(kind));
                }
                let bank = bank as u32;
                if banks.contains(&bank) {
                    Ok(bank)
                } else {
                    Err(AsmErrorKind::BankOutOfRange(
                        bank,
                        *banks.start(),
                        *banks.end(),
                    ))
                }
            },
            symbols,
            macro_args,
            sections,
        )?;
        let mut alignment = eval(
            attrs.alignment,
            |alignment| {
                if matches!(alignment, 0..=16) {
                    Ok(alignment as u8)
                } else {
                    Err(AsmErrorKind::AlignOutOfRange(alignment))
                }
            },
            symbols,
            macro_args,
            sections,
        )?
        .unwrap_or(0);
        let align_offset = eval(
            attrs.offset,
            |offset| {
                if offset >= 0 && offset < 1 << alignment {
                    Ok(offset as u16)
                } else {
                    Err(AsmErrorKind::AlignOfsOutOfRange(offset, 1 << alignment))
                }
            },
            symbols,
            macro_args,
            sections,
        )?
        .unwrap_or(0);

        // Now, perform some more checks.

        if alignment != 0 {
            debug_assert!(alignment <= 16);
            let mask = u16::MAX >> (16 - alignment);

            if let Some(addr) = address {
                if (addr & mask) != align_offset {
                    return Err(AsmError {
                        begin: def_begin,
                        end: def_end,
                        kind: AsmErrorKind::AlignMismatch(addr, alignment, align_offset),
                    }
                    .into());
                }
                alignment = 0; // Ignore alignment if the address already satisfies it.
            } else if start_addr & mask != 0 {
                return Err(AsmError {
                    begin: def_begin,
                    end: def_end,
                    kind: AsmErrorKind::OverAligned(alignment, kind),
                }
                .into());
            } else if alignment == 16 {
                alignment = 0;
                address = Some(16);
            }
        }

        let first_bank = *banks.start();
        if first_bank == *banks.end() {
            if let Some(bank) = bank {
                debug_assert_eq!(bank, first_bank);
            }
            bank = Some(first_bank);
        }

        Ok((
            Self {
                address,
                bank,
                alignment,
                align_offset,
            },
            // "Return" the locations, since they weren't used.
            def_begin,
            def_end,
        ))
    }
}