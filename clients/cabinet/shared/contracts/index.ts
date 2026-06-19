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
  BankingV1WalletNetwork as WalletNetwork,
  BankingV1DepositAddress as DepositAddress,
  BankingV1Withdrawal as Withdrawal,
  BankingV1WithdrawalList as WithdrawalList,
  BankingV1RequestWithdrawalRequest as RequestWithdrawalRequest,
  BankingV1Allocation as Allocation,
  BankingV1AllocationList as AllocationList,
  BankingV1AllocateRequest as AllocateRequest,
  BankingV1Sharer as Sharer,
  BankingV1UserProfile as UserProfile,
  BankingV1UserBalanceResponse as UserBalanceResponse,
  BankingV1FundBalance as FundBalance,
  BankingV1NetworkBalance as NetworkBalance,
} from "./gen";
