use core::convert::TryFrom;
use core::mem::size_of;
use pinocchio::{
    account_info::AccountInfo,
    instruction::{Seed, Signer},
    program_error::ProgramError,
    pubkey::{find_program_address, Pubkey},
    sysvars::{rent::Rent, Sysvar},
    ProgramResult,
};
use pinocchio_log::log;
use pinocchio_system::instructions::{CreateAccount, Transfer as SystemTransfer};
use shank::ShankInstruction;


#[derive(ShankInstruction)]
pub enum _Instruction {
    #[account(
        0,
        name = "owner",
        writable,
        signer,
        desc = "signer of the vault tx  and vault owner"
    )]
    #[account(1, name = "vault", writable, desc = "the vault account itself")]
    #[account(2, name = "program", desc = "Program address")]
    #[account(3, name = "system_program", desc = "system program address")]
    Deposit { amount: u64 },

    #[account(
        0,
        signer,
        writable,
        name = "owner",
        desc = "Vault owner and authority"
    )]
    #[account(1, writable, name = "vault", desc = "Vault PDA itself")]
    #[account(2, name = "program", desc = "Program Address")]
    Withdraw {},
}

fn parse_amount(data: &[u8]) -> Result<u64, ProgramError> {
    if data.len() != size_of::<u64>() {
        return Err(ProgramError::InvalidInstructionData);
    }

    let amount = u64::from_le_bytes(data.try_into().unwrap());

    if amount == 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(amount)
}

fn derive_vault_pda(owner: &AccountInfo) -> (Pubkey, u8) {
    find_program_address(&[b"no-std-vault", owner.key().as_ref()], &crate::ID)
}

fn check_vault_existence(owner: &AccountInfo, vault: &AccountInfo) -> ProgramResult {
    if !owner.is_signer() {
        return Err(ProgramError::InvalidAccountOwner);
    }

    if vault.lamports() == 0 {
        const DISCRIMINATOR: usize = 8;

        let (_pda, bump) = derive_vault_pda(owner);

        let seeds = [
            Seed::from(b"vault".as_ref()),
            Seed::from(owner.key().as_ref()),
            Seed::from(core::slice::from_ref(&bump)),
        ];

        let signer = Signer::from(&seeds);

        let data_len: usize = DISCRIMINATOR + size_of::<u64>();

        let required_lamports = Rent::get()?.minimum_balance(data_len);

        CreateAccount {
            from: owner,
            to: vault,
            lamports: required_lamports,
            space: data_len as u64,
            owner: &crate::ID,
        }
        .invoke_signed(&[signer])?;

        log!("Vault now active on chain!");
    } else {
        if !vault.is_owned_by(&crate::ID) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        log!("Vault already exists!");
    }
    Ok(())
}

pub struct Deposit<'a> {
    pub owner: &'a AccountInfo,
    pub vault: &'a AccountInfo,
    pub amount: u64,
}

impl<'a> Deposit<'a> {
    pub const DISCRIMINATOR: &'a u8 = &0;
    pub fn process(self) -> ProgramResult {
        let Deposit {
            owner,
            vault,
            amount,
        } = self;

        check_vault_existence(owner, vault)?;

        SystemTransfer {
            from: owner,
            to: vault,
            lamports: amount,
        }
        .invoke()?;

        log!(" {} funds moved to vault!", amount);

        Ok(())
    }
}

impl<'a> TryFrom<(&'a [u8], &'a [AccountInfo])> for Deposit<'a> {
    type Error = ProgramError;

    fn try_from(value: (&'a [u8], &'a [AccountInfo])) -> Result<Self, Self::Error> {
        let (data, accounts) = value;

        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let owner = &accounts[0];
        let vault = &accounts[1];
        let amount = parse_amount(data)?;

        Ok(Self {
            owner,
            vault,
            amount,
        })
    }
}

pub struct Withdraw<'a> {
    pub owner: &'a AccountInfo,
    pub vault: &'a AccountInfo,
}

impl<'a> Withdraw<'a> {
    pub const DISCRIMINATOR: &'a u8 = &1;

    pub fn process(self) -> ProgramResult {
        let Withdraw { owner, vault } = self;

        if !owner.is_signer() {
            return Err(ProgramError::InvalidAccountOwner);
        }

        if !vault.is_owned_by(&crate::ID) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        let (expected_vault_pda, _bump) = derive_vault_pda(owner);

        if vault.key() != &expected_vault_pda {
            return Err(ProgramError::InvalidAccountData);
        }

        let data_len = vault.data_len();
        let minimum_bal = Rent::get()?.minimum_balance(data_len);
        let current_balance = vault.lamports();

        if current_balance <= minimum_bal {
            return Err(ProgramError::InsufficientFunds);
        }

        let amount = current_balance - minimum_bal;

        {
            let mut vault_lamports = vault.try_borrow_mut_lamports()?;

            *vault_lamports = vault_lamports
                .checked_sub(amount)
                .ok_or(ProgramError::InsufficientFunds)?;
        }

        {
            let mut owner_lamports = vault.try_borrow_mut_lamports()?;
            *owner_lamports = owner_lamports
                .checked_add(amount)
                .ok_or(ProgramError::InsufficientFunds)?;
        }

        log!("{} lamports withdrawn from vault", amount);

        Ok(())
    }
}

impl<'a> TryFrom<&'a [AccountInfo]> for Withdraw<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountInfo]) -> Result<Self, Self::Error> {
        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
        let owner = &accounts[0];
        let vault = &accounts[1];
        Ok(Self { owner, vault })
    }
}
