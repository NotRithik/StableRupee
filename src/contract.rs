use core::panic;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env,
    MessageInfo, Response, StdResult, Uint128, WasmMsg, WasmQuery,
};

use cw2::set_contract_version;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{
    ADMIN_ADDRESSES, COLLATERAL_TOKEN_DENOM, COLLATERAL_TOKEN_PRICE, LIQUIDATION_HEALTH,
    LOCKED_COLLATERAL, MINTABLE_HEALTH, MINTED_DIRA,
};

use cw20::{Cw20ExecuteMsg, Cw20QueryMsg};
use cw721::{Cw721ExecuteMsg, Cw721QueryMsg};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:cosmwasm-stable-rupee";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

/****
 * THIS IS THE SECTION FOR MATCHING EXECUTE AND QUERY MESSAGES
 * FROM msg.rs IN HERE. THE ACTUAL FUNCTION IMPLEMENTATIONS ARE DONE IN THE SECTION
 * WAY BELOW THIS ONE
 ****/

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
    // liquidation_health: f32,
    // allowed_collaterals: Vec<CollateralToken>,
) -> Result<Response, ContractError> {
    deps.api.debug("Instantiating contract...");
    deps.api.debug(&format!("Received message: {:?}", msg));

    if msg.liquidation_health.is_zero() || msg.mintable_health.is_zero() {
        return Err(ContractError::HealthCannotBeZero {});
    }

    if msg.collateral_token_denom.is_empty() {
        return Err(ContractError::MissingCollateralTokenDenom {});
    }

    if msg.mintable_health < msg.liquidation_health {
        return Err(ContractError::MintableHealthLowerThanLiquidationHealth {});
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    ADMIN_ADDRESSES.save(deps.storage, &vec![info.sender.clone()])?;
    LIQUIDATION_HEALTH.save(deps.storage, &msg.liquidation_health)?;
    MINTABLE_HEALTH.save(deps.storage, &msg.mintable_health)?;
    COLLATERAL_TOKEN_DENOM.save(deps.storage, &msg.collateral_token_denom)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("admin", info.sender))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    deps.api.debug("Executing function...");
    deps.api.debug(&format!("Received message: {:?}", &msg));

    match msg {
        ExecuteMsg::LockCollateral {
            collateral_amount_to_lock,
        } => execute_lock_collateral(deps, info, collateral_amount_to_lock),

        ExecuteMsg::UnlockCollateral {
            collateral_amount_to_unlock,
        } => execute_unlock_collateral(deps, info, collateral_amount_to_unlock),

        ExecuteMsg::MintDira { dira_to_mint } => {
            execute_mint_dira(deps, info.sender.into_string(), dira_to_mint)
        }
        ExecuteMsg::RedeemDira { dira_to_redeem } => {
            execute_return_dira(deps, info.sender.into_string(), dira_to_redeem)
        }

        ExecuteMsg::LiquidateStablecoins {
            liquidate_stablecoin_minter_address,
        } => execute_liquidate_stablecoin_minter(
            deps,
            info.sender.into_string(),
            liquidate_stablecoin_minter_address,
        ),

        ExecuteMsg::SetCollateralPriceInDirham {
            collateral_price_in_aed,
        } => execute_set_collateral_price_in_dirham(
            deps,
            info.sender.into_string(),
            collateral_price_in_aed,
        ),

        ExecuteMsg::SetLiquidationHealth { liquidation_health } => {
            execute_set_liquidation_health(deps, info.sender.into_string(), liquidation_health)
        }
        ExecuteMsg::SetMintableHealth { mintable_health } => {
            execute_set_mintable_health(deps, info.sender.into_string(), mintable_health)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::QueryCollateralPrice {} => query_collateral_price(&deps),

        QueryMsg::QueryLockedCollateral {
            wallet_address_to_query,
        } => query_locked_collateral(&deps, wallet_address_to_query),

        QueryMsg::QueryStablecoinHealth {
            stablecoin_minter_address_to_query,
        } => query_stablecoin_health(&deps, stablecoin_minter_address_to_query),
    }
}

/****
 * THIS IS THE SECTION FOR ACTUAL IMPLEMENTATIONS OF ALL THE FUNCTIONS USED ABOVE!
 ****/

// Function to calculate stablecoin health of a particular user
// based on how much stablecoin they've minted and how much
// collateral they have locked
fn calculate_stablecoin_health(
    minted_dira: Decimal,
    locked_collateral: Decimal,
    collateral_price_in_aed: Decimal,
) -> Decimal {
    let locked_collateral_value_in_aed = collateral_price_in_aed * locked_collateral;

    if minted_dira.is_zero() {
        if !locked_collateral_value_in_aed.is_zero() {
            return Decimal::zero();
        } else {
            return Decimal::MAX;
        }
    }

    return locked_collateral_value_in_aed / minted_dira;
}

// Function to calculate how much Dira the user can mint
// based on how much collateral is locked, what the
// value of the collateral is and what the
// mintable health is
fn calculate_max_mintable_dira(
    locked_collateral: Decimal,
    collateral_price_in_aed: Decimal,
    mintable_health: Decimal,
) -> Decimal {
    let max_mintable_dira = (locked_collateral * collateral_price_in_aed) / mintable_health;

    max_mintable_dira
}

// Function to calculate how much collateral can be unlocked
// based on how much Dira the user has minted, what the value
// of the collateral is, and what the liquidation health is
fn calculate_max_unlockable_collateral(
    locked_collateral: Decimal,
    collateral_price_in_aed: Decimal,
    minted_dira: Decimal,
    mintable_health: Decimal,
) -> Decimal {
    let required_collateral_for_minted_dira =
        (minted_dira * mintable_health) / collateral_price_in_aed;
    let unlockable_collateral = locked_collateral - required_collateral_for_minted_dira;

    unlockable_collateral
}

// Function to lock collateral
fn execute_lock_collateral(
    deps: DepsMut,
    info: MessageInfo,
    // env: Env,
    collateral_amount: Decimal,
) -> Result<Response, ContractError> {
    let collateral_token_denom = COLLATERAL_TOKEN_DENOM
        .load(deps.storage)
        .map_err(|_| ContractError::MissingCollateralTokenDenom {})?;

    let message_sender = info.sender;

    // Check if the user has sent enough funds along with the transaction
    let sent_funds = info
        .funds
        .iter()
        .find(|coin| coin.denom == collateral_token_denom)
        .ok_or(ContractError::InsufficientFundsSent {})
        .unwrap();

    let sent_amount = Decimal::from_ratio(sent_funds.amount, Uint128::new(1));

    if sent_amount < collateral_amount {
        return Err(ContractError::InsufficientFundsSent {});
    }

    match LOCKED_COLLATERAL.update(
        deps.storage,
        message_sender.clone(),
        |balance: Option<Decimal>| -> Result<Decimal, ContractError> {
            Ok(balance.unwrap_or_default() + collateral_amount)
        },
    ) {
        Ok(_result) => {}
        Err(error) => {
            dbg!("Error in updating LOCKED_COLLATERAL storage item");
            return Err(error);
        }
    };

    // Send the lock collateral messages and return the Ok response
    Ok(Response::new()
        .add_attribute("action", "lock_collateral")
        .add_attribute("sender", message_sender.clone())
        .add_attribute(
            "total_funds_locked_by_user",
            LOCKED_COLLATERAL
                .load(deps.storage, message_sender)
                .unwrap_or_default()
                .to_string(),
        ))
}

// Function to unlock collateral
fn execute_unlock_collateral(
    deps: DepsMut,
    info: MessageInfo,
    collateral_amount: Decimal,
) -> Result<Response, ContractError> {
    let collateral_token_denom = COLLATERAL_TOKEN_DENOM
        .load(deps.storage)
        .map_err(|_| ContractError::MissingCollateralTokenDenom {})?;

    let message_sender = info.sender;

    let locked_collateral = LOCKED_COLLATERAL
        .load(deps.storage, message_sender.clone())
        .unwrap_or_default();

    let minted_dira = MINTED_DIRA
        .load(deps.storage, message_sender.clone())
        .unwrap_or_default();

    let mintable_health = MINTABLE_HEALTH.load(deps.storage)?;

    let collateral_price_in_aed = COLLATERAL_TOKEN_PRICE
        .may_load(deps.storage)?
        .ok_or(ContractError::CollateralPriceNotSet {})
        .unwrap();

    let max_unlockable_collateral = calculate_max_unlockable_collateral(
        locked_collateral,
        collateral_price_in_aed,
        minted_dira,
        mintable_health,
    );

    if collateral_amount > max_unlockable_collateral {
        return Err(ContractError::UnlockAmountTooHigh {
            max_unlockable: max_unlockable_collateral,
        });
    }

    match LOCKED_COLLATERAL.update(
        deps.storage,
        message_sender.clone(),
        |balance: Option<Decimal>| -> Result<Decimal, ContractError> {
            match balance {
                Some(bal) => {
                    if bal < collateral_amount {
                        return Err(ContractError::InsufficientCollateral {});
                    }
                    Ok(bal - collateral_amount)
                }
                None => Err(ContractError::InsufficientCollateral {}),
            }
        },
    ) {
        Ok(_result) => {}
        Err(error) => {
            return Err(error);
        }
    }

    let unlock_collateral_message = BankMsg::Send {
        to_address: message_sender.to_string(),
        amount: vec![Coin {
            denom: collateral_token_denom,
            amount: collateral_amount * Uint128::new(1), // Assuming decimal representation
        }],
    };

    Ok(Response::new().add_message(unlock_collateral_message))
}

// Function to mint rupees
fn execute_mint_dira(
    deps: DepsMut,
    sender: String,
    dira_to_mint: Decimal,
) -> Result<Response, ContractError> {
    panic!("TODO: Implement this function!");
}

// Function to return rupees
fn execute_return_dira(
    deps: DepsMut,
    sender: String,
    dira_to_return: Decimal,
) -> Result<Response, ContractError> {
    panic!("TODO: Implement this function!");
}

// Function to liquidate stablecoins
fn execute_liquidate_stablecoin_minter(
    deps: DepsMut,
    sender: String,
    liquidate_stablecoin_minter_address: String,
) -> Result<Response, ContractError> {
    panic!("TODO: Implement this function!");
}

// Function to set collateral prices in rupees
fn execute_set_collateral_price_in_dirham(
    deps: DepsMut,
    sender: String,
    collateral_price_in_aed: Decimal,
) -> Result<Response, ContractError> {
    let admins = ADMIN_ADDRESSES.load(deps.storage)?;
    let sender_address = deps.api.addr_validate(&sender)?;

    if !admins.contains(&sender_address) {
        return Err(ContractError::UnauthorizedUser {});
    }

    match COLLATERAL_TOKEN_PRICE.save(deps.storage, &collateral_price_in_aed) {
        Ok(_result) => {}
        Err(error) => {
            dbg!(&error);
            panic!("Error in updating COLLATERAL_TOKEN_PRICE storage item");
        }
    }

    Ok(Response::new()
        .add_attribute("action", "set_collateral_price_in_dirham")
        .add_attribute("sender", sender)
        .add_attribute("new_collateral_price", collateral_price_in_aed.to_string()))
}

// Function to set liquidation health
fn execute_set_liquidation_health(
    deps: DepsMut,
    sender: String,
    liquidation_health: Decimal,
) -> Result<Response, ContractError> {
    let admins = ADMIN_ADDRESSES.load(deps.storage)?;
    let sender_address = deps.api.addr_validate(&sender)?;

    if !admins.contains(&sender_address) {
        return Err(ContractError::UnauthorizedUser {});
    }

    LIQUIDATION_HEALTH.update(
        deps.storage,
        |_current_liquidation_health| -> Result<Decimal, ContractError> { Ok(liquidation_health) },
    )?;

    Ok(Response::new()
        .add_attribute("action", "set_liquidation_health")
        .add_attribute("sender", sender)
        .add_attribute("new_liquidation_health", liquidation_health.to_string()))
}

//
fn execute_set_mintable_health(
    deps: DepsMut,
    sender: String,
    mintable_health: Decimal,
) -> Result<Response, ContractError> {
    let admins = ADMIN_ADDRESSES.load(deps.storage)?;
    let sender_address = deps.api.addr_validate(&sender)?;

    if !admins.contains(&sender_address) {
        return Err(ContractError::UnauthorizedUser {});
    }

    let current_liquidation_health = LIQUIDATION_HEALTH.load(deps.storage)?;

    MINTABLE_HEALTH.update(
        deps.storage,
        |_current_mintable_health| -> Result<Decimal, ContractError> {
            if mintable_health < current_liquidation_health {
                return Err(ContractError::MintableHealthLowerThanLiquidationHealth {});
            } else {
                return Ok(mintable_health);
            }
        },
    )?;

    Ok(Response::new()
        .add_attribute("action", "set_mintable_health")
        .add_attribute("sender", sender)
        .add_attribute("new_liquidation_health", mintable_health.to_string()))
}

// Query function to get collateral prices
fn query_collateral_price(deps: &Deps) -> StdResult<Binary> {
    panic!("TODO: Implement this function!");
}

// Query function to get locked collateral
fn query_locked_collateral(deps: &Deps, collateral_address_to_query: Addr) -> StdResult<Binary> {
    panic!("TODO: Implement this function!");
}

// Query function to get stablecoin health
fn query_stablecoin_health(
    deps: &Deps,
    stablecoin_minter_address_to_query: Addr,
) -> StdResult<Binary> {
    panic!("TODO: Implement this function!");
}
