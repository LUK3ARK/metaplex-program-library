use {
    crate::{
        error::MetaplexError,
        state::{
            FractionManager, FractionManagerStatus, FractionManagerV1, FractionSafetyDepositConfig,
            FractionWinningConfigType, Key, OriginalAuthorityLookup, Store,
            MAX_AUTHORITY_LOOKUP_SIZE, PREFIX,
        },
        utils::{
            assert_at_least_one_fraction_creator_matches_or_store_public_and_all_verified,
            assert_authority_correct, assert_derivation, assert_initialized, assert_owned_by,
            assert_store_safety_vault_manager_match, create_or_allocate_account_raw,
            transfer_metadata_ownership,
        },
    },
    borsh::BorshSerialize,
    mpl_token_metadata::{
        state::{MasterEditionV1, MasterEditionV2, Metadata},
        utils::assert_update_authority_is_correct,
    },
    mpl_token_vault::state::{SafetyDepositBox, Vault},
    solana_program::{
        account_info::{next_account_info, AccountInfo},
        entrypoint::ProgramResult,
        pubkey::Pubkey,
    },
    spl_token::state::{Account, Mint},
};
pub fn make_fraction_safety_deposit_config<'a>(
    program_id: &Pubkey,
    fraction_manager_info: &AccountInfo<'a>,
    safety_deposit_info: &AccountInfo<'a>,
    safety_deposit_config_info: &AccountInfo<'a>,
    payer_info: &AccountInfo<'a>,
    rent_info: &AccountInfo<'a>,
    system_info: &AccountInfo<'a>,
    safety_deposit_config: &FractionSafetyDepositConfig,
) -> ProgramResult {
    let bump = assert_derivation(
        program_id,
        safety_deposit_config_info,
        &[
            PREFIX.as_bytes(),
            program_id.as_ref(),
            fraction_manager_info.key.as_ref(),
            safety_deposit_info.key.as_ref(),
        ],
    )?;

    create_or_allocate_account_raw(
        *program_id,
        safety_deposit_config_info,
        rent_info,
        system_info,
        payer_info,
        safety_deposit_config.created_size(),
        &[
            PREFIX.as_bytes(),
            program_id.as_ref(),
            fraction_manager_info.key.as_ref(),
            safety_deposit_info.key.as_ref(),
            &[bump],
        ],
    )?;

    safety_deposit_config.create(safety_deposit_config_info, fraction_manager_info.key)?;

    Ok(())
}

pub struct CommonCheckArgs<'a, 'b> {
    pub program_id: &'a Pubkey,
    pub fraction_manager_info: &'a AccountInfo<'a>,
    pub metadata_info: &'a AccountInfo<'a>,
    pub original_authority_lookup_info: &'a AccountInfo<'a>,
    pub whitelisted_creator_info: &'a AccountInfo<'a>,
    pub safety_deposit_info: &'a AccountInfo<'a>,
    pub safety_deposit_token_store_info: &'a AccountInfo<'a>,
    pub edition_info: &'a AccountInfo<'a>,
    pub vault_info: &'a AccountInfo<'a>,
    pub mint_info: &'a AccountInfo<'a>,
    pub token_metadata_program_info: &'a AccountInfo<'a>,
    pub fraction_manager_store_info: &'a AccountInfo<'a>,
    pub authority_info: &'a AccountInfo<'a>,
    pub store: &'b Store,
    pub fraction_manager: &'b dyn FractionManager,
    pub metadata: &'b Metadata,
    pub safety_deposit: &'b SafetyDepositBox,
    pub vault: &'b Vault,
    pub winning_config_type: &'b FractionWinningConfigType,
}

pub fn assert_common_checks(args: CommonCheckArgs) -> ProgramResult {
    let CommonCheckArgs {
        program_id,
        fraction_manager_info,
        metadata_info,
        original_authority_lookup_info,
        whitelisted_creator_info,
        safety_deposit_info,
        safety_deposit_token_store_info,
        edition_info,
        vault_info,
        mint_info,
        token_metadata_program_info,
        fraction_manager_store_info,
        authority_info,
        store,
        fraction_manager,
        metadata,
        safety_deposit,
        vault,
        winning_config_type,
    } = args;

    // Is it a real mint?
    let _mint: Mint = assert_initialized(mint_info)?;

    if vault.authority != *fraction_manager_info.key {
        return Err(MetaplexError::VaultAuthorityMismatch.into());
    }

    assert_owned_by(fraction_manager_info, program_id)?;
    assert_owned_by(metadata_info, &store.token_metadata_program)?;
    if !original_authority_lookup_info.data_is_empty() {
        return Err(MetaplexError::AlreadyInitialized.into());
    }

    if *whitelisted_creator_info.key != solana_program::system_program::id() {
        if whitelisted_creator_info.data_is_empty() {
            return Err(MetaplexError::Uninitialized.into());
        }
        assert_owned_by(whitelisted_creator_info, program_id)?;
    }

    assert_owned_by(fraction_manager_store_info, program_id)?;
    assert_owned_by(safety_deposit_info, &store.token_vault_program)?;
    assert_owned_by(safety_deposit_token_store_info, &store.token_program)?;
    assert_owned_by(mint_info, &store.token_program)?;

    if *winning_config_type != FractionWinningConfigType::FractionToken {
        assert_owned_by(edition_info, &store.token_metadata_program)?;
    }
    assert_owned_by(vault_info, &store.token_vault_program)?;

    if *token_metadata_program_info.key != store.token_metadata_program {
        return Err(MetaplexError::FractionManagerTokenMetadataMismatch.into());
    }

    assert_authority_correct(&fraction_manager.authority(), authority_info)?;
    assert_store_safety_vault_manager_match(
        &fraction_manager.vault(),
        &safety_deposit_info,
        vault_info,
        &store.token_vault_program,
    )?;
    assert_at_least_one_fraction_creator_matches_or_store_public_and_all_verified(
        program_id,
        fraction_manager,
        &metadata,
        whitelisted_creator_info,
        fraction_manager_store_info,
    )?;

    if fraction_manager.store() != *fraction_manager_store_info.key {
        return Err(MetaplexError::FractionManagerStoreMismatch.into());
    }

    if *mint_info.key != safety_deposit.token_mint {
        return Err(MetaplexError::SafetyDepositBoxMintMismatch.into());
    }

    if *token_metadata_program_info.key != store.token_metadata_program {
        return Err(MetaplexError::FractionManagerTokenMetadataProgramMismatch.into());
    }

    // We want to ensure that the mint you are using with this token is one
    // we can actually transfer to and from using our token program invocations, which
    // we can check by asserting ownership by the token program we recorded in init.
    if *mint_info.owner != store.token_program {
        return Err(MetaplexError::TokenProgramMismatch.into());
    }

    Ok(())
}

pub struct SupplyLogicCheckArgs<'a, 'b> {
    pub program_id: &'a Pubkey,
    pub fraction_manager_info: &'a AccountInfo<'a>,
    pub metadata_info: &'a AccountInfo<'a>,
    pub edition_info: &'a AccountInfo<'a>,
    pub metadata_authority_info: &'a AccountInfo<'a>,
    pub original_authority_lookup_info: &'a AccountInfo<'a>,
    pub rent_info: &'a AccountInfo<'a>,
    pub system_info: &'a AccountInfo<'a>,
    pub payer_info: &'a AccountInfo<'a>,
    pub token_metadata_program_info: &'a AccountInfo<'a>,
    pub safety_deposit_token_store_info: &'a AccountInfo<'a>,
    pub fraction_manager: &'b dyn FractionManager,
    pub winning_config_type: &'b FractionWinningConfigType,
    pub metadata: &'b Metadata,
    pub safety_deposit: &'b SafetyDepositBox,
    pub store: &'b Store,
}

pub fn assert_supply_logic_check(args: SupplyLogicCheckArgs) -> ProgramResult {
    let SupplyLogicCheckArgs {
        program_id,
        fraction_manager_info,
        metadata_info,
        edition_info,
        metadata_authority_info,
        original_authority_lookup_info,
        rent_info,
        system_info,
        payer_info,
        token_metadata_program_info,
        fraction_manager,
        winning_config_type,
        metadata,
        safety_deposit,
        store,
        safety_deposit_token_store_info,
    } = args;

    let safety_deposit_token_store: Account = assert_initialized(safety_deposit_token_store_info)?;

    let edition_seeds = &[
        mpl_token_metadata::state::PREFIX.as_bytes(),
        store.token_metadata_program.as_ref(),
        &metadata.mint.as_ref(),
        mpl_token_metadata::state::EDITION.as_bytes(),
    ];

    let (edition_key, _) =
        Pubkey::find_program_address(edition_seeds, &store.token_metadata_program);

    // HERE IS A POINT IT CAN BREAK
    // remember, seeds are used as a definition of what is correct.
    // when the transaction is signed, it is verified that the supplied signers is equal to something this generates??
    // TODO authority seeds might not be vault, may need...
    let vault_key = fraction_manager.vault();
    let seeds = &[PREFIX.as_bytes(), vault_key.as_ref()];
    let (_, bump_seed) = Pubkey::find_program_address(seeds, &program_id);

    let authority_seeds = &[PREFIX.as_bytes(), vault_key.as_ref(), &[bump_seed]];

    // Supply logic check
    match winning_config_type {
        FractionWinningConfigType::FractionMasterEditionV2 => {
            // Asserts current wallet owner is the correct metadata owner
            assert_update_authority_is_correct(&metadata, metadata_authority_info)?;

            if safety_deposit.token_mint != metadata.mint {
                return Err(MetaplexError::SafetyDepositBoxMetadataMismatch.into());
            }
            if edition_key != *edition_info.key {
                return Err(MetaplexError::InvalidEditionAddress.into());
            }

            if safety_deposit_token_store.amount != 1 {
                return Err(MetaplexError::StoreIsEmpty.into());
            }

            // TODO - IS THIS NEEDED!!!!!!!!
            // if total_amount_requested != 1 {
            //     return Err(MetaplexError::NotEnoughTokensToSupplyVaultBuyer.into());
            // }

            let vault_key = fraction_manager.vault();

            // MAKES THE SEEDS OF WHAT SHOULD BE MADE WITH 'original_authority_lookup_info'
            //
            // TODO FINISH FRACTION MANAGER AND HOW AUTHORITY IS DERIVED AND THEN COPY HERE
            // just have tiny think here!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
            // AUTHORITY IS PROGRAM DERIVED I THINK?
            let original_authority_lookup_seeds = &[
                PREFIX.as_bytes(),
                vault_key.as_ref(),
                metadata_info.key.as_ref(),
            ];

            // See here, original_authority_seeds is same as original_authority_lookup_seeds + bump seed we just found :)
            let (expected_key, original_bump_seed) =
                Pubkey::find_program_address(original_authority_lookup_seeds, &program_id);
            let original_authority_seeds = &[
                PREFIX.as_bytes(),
                vault_key.as_ref(),
                metadata_info.key.as_ref(),
                &[original_bump_seed],
            ];

            // THIS IS USING PDA TO VERIFY!!
            if expected_key != *original_authority_lookup_info.key {
                return Err(MetaplexError::FractionOriginalAuthorityLookupKeyMismatch.into());
            }

            // We may need to transfer authority back, or to the new owner, so we need to keep track
            // of original ownership
            create_or_allocate_account_raw(
                *program_id,
                original_authority_lookup_info,
                rent_info,
                system_info,
                payer_info,
                MAX_AUTHORITY_LOOKUP_SIZE,
                original_authority_seeds,
            )?;

            let mut original_authority_lookup =
                OriginalAuthorityLookup::from_account_info(original_authority_lookup_info)?;
            original_authority_lookup.key = Key::OriginalAuthorityLookupV1;

            original_authority_lookup.original_authority = *metadata_authority_info.key;

            // Transfers the ownership of the metadata (for the picture I believe)
            // from the current authority (the connected wallet!) -> to the fraction manager while this is fractionalised
            transfer_metadata_ownership(
                token_metadata_program_info.clone(),
                metadata_info.clone(),
                metadata_authority_info.clone(),
                fraction_manager_info.clone(),
                authority_seeds,
            )?;

            original_authority_lookup
                .serialize(&mut *original_authority_lookup_info.data.borrow_mut())?;
        }
        FractionWinningConfigType::FractionToken => {
            if safety_deposit.token_mint != metadata.mint {
                return Err(MetaplexError::SafetyDepositBoxMetadataMismatch.into());
            }
            // todo - same as above
            // if safety_deposit_token_store.amount < total_amount_requested {
            //     return Err(MetaplexError::NotEnoughTokensToSupplyVaultBuyer.into());
            // }
        }
    }

    Ok(())
}

pub fn process_validate_fraction_safety_deposit_box<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    safety_deposit_config: FractionSafetyDepositConfig,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let safety_deposit_config_info = next_account_info(account_info_iter)?;
    let mut fraction_manager_info = next_account_info(account_info_iter)?;
    let metadata_info = next_account_info(account_info_iter)?;
    let original_authority_lookup_info = next_account_info(account_info_iter)?;
    let whitelisted_creator_info = next_account_info(account_info_iter)?;
    // This is the actual store info to give to manager to get paid i think
    let fraction_manager_store_info = next_account_info(account_info_iter)?;
    let safety_deposit_info = next_account_info(account_info_iter)?;
    // !!!!! these are just the actual stores (public key references)
    let safety_deposit_token_store_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let edition_info = next_account_info(account_info_iter)?;
    let vault_info = next_account_info(account_info_iter)?;
    let authority_info = next_account_info(account_info_iter)?;
    let metadata_authority_info = next_account_info(account_info_iter)?;
    let payer_info = next_account_info(account_info_iter)?;
    let token_metadata_program_info = next_account_info(account_info_iter)?;
    let system_info = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;

    if !safety_deposit_config_info.data_is_empty() {
        return Err(MetaplexError::AlreadyValidated.into());
    }

    // get fraction manager from account info
    let mut fraction_manager = FractionManagerV1::from_account_info(fraction_manager_info)?;
    let safety_deposit = SafetyDepositBox::from_account_info(safety_deposit_info)?;
    let metadata = Metadata::from_account_info(metadata_info)?;
    let store = Store::from_account_info(fraction_manager_store_info)?;
    // Is it a real vault?
    let vault = Vault::from_account_info(vault_info)?;

    assert_common_checks(CommonCheckArgs {
        program_id,
        fraction_manager_info,
        metadata_info,
        original_authority_lookup_info,
        whitelisted_creator_info,
        safety_deposit_info,
        safety_deposit_token_store_info,
        edition_info,
        vault_info,
        mint_info,
        token_metadata_program_info,
        fraction_manager_store_info,
        authority_info,
        store: &store,
        fraction_manager: &fraction_manager,
        metadata: &metadata,
        safety_deposit: &safety_deposit,
        vault: &vault,
        winning_config_type: &safety_deposit_config.fraction_winning_config_type,
    })?;

    assert_supply_logic_check(SupplyLogicCheckArgs {
        program_id,
        fraction_manager_info,
        metadata_info,
        edition_info,
        metadata_authority_info,
        original_authority_lookup_info,
        rent_info,
        system_info,
        payer_info,
        token_metadata_program_info,
        fraction_manager: &fraction_manager,
        winning_config_type: &safety_deposit_config.fraction_winning_config_type,
        metadata: &metadata,
        safety_deposit: &safety_deposit,
        store: &store,
        safety_deposit_token_store_info,
    })?;

    if safety_deposit_config.order != safety_deposit.order as u64 {
        return Err(MetaplexError::SafetyDepositConfigOrderMismatch.into());
    }

    fraction_manager.state.safety_config_items_validated = fraction_manager
        .state
        .safety_config_items_validated
        .checked_add(1)
        .ok_or(MetaplexError::NumericalOverflowError)?;

    if fraction_manager.state.safety_config_items_validated == vault.token_type_count as u64 {
        fraction_manager.state.status = FractionManagerStatus::Validated
    }

    fraction_manager.save(&mut fraction_manager_info)?;

    make_fraction_safety_deposit_config(
        program_id,
        fraction_manager_info,
        safety_deposit_info,
        safety_deposit_config_info,
        payer_info,
        rent_info,
        system_info,
        &safety_deposit_config,
    )?;
    Ok(())
}
