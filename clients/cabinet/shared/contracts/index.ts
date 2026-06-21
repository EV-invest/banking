// Clean re-exports of the proto-derived types — import from `@/shared/contracts`,
// never `./gen` directly.
//
// The backend gRPC proto (`contracts/proto`) is the single source of truth:
// `protoc-gen-connect-openapi` emits `contracts/openapi.json` from it, and
// `@hey-api/openapi-ts` emits `./gen` from that (`npm run gen:api`, or the flake
// `nix run .#gen-api`). Field names are snake_case to match the BFF's `keepCase`
// proto-loader shape, and every field is optional (proto3 JSON semantics).

export type {
  BankingV1Wallet as Wallet,
  BankingV1Balance as Balance,
  BankingV1NetworkWithdrawable as NetworkWithdrawable,
  BankingV1DepositAddress as DepositAddress,
  BankingV1Withdrawal as Withdrawal,
  BankingV1WithdrawalList as WithdrawalList,
  BankingV1RequestWithdrawalRequest as RequestWithdrawalRequest,
  BankingV1CancelWithdrawalRequest as CancelWithdrawalRequest,
  BankingV1UserProfile as UserProfile,
  BankingV1UserBalanceResponse as UserBalanceResponse,
  BankingV1Treasury as Treasury,
  BankingV1RailLiquidity as RailLiquidity,
  // Fund shares (the service currency).
  BankingV1Position as Position,
  BankingV1PositionList as PositionList,
  BankingV1FundNav as FundNav,
  BankingV1Subscription as Subscription,
  BankingV1SubscribeRequest as SubscribeRequest,
  BankingV1RedeemRequest as RedeemRequest,
  BankingV1Redemption as Redemption,
  BankingV1RedemptionList as RedemptionList,
  BankingV1CancelRedemptionRequest as CancelRedemptionRequest,
} from "./gen";
