{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
    pre-commit-hooks.url = "github:cachix/git-hooks.nix";
    v_flakes.url = "github:valeratrades/v_flakes?ref=v1.6";
  };
  outputs = { self, nixpkgs, rust-overlay, flake-utils, pre-commit-hooks, v_flakes }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
          allowUnfree = true;
        };
        #NB: can't load rust-bin from nightly.latest, as there are week guarantees of which components will be available on each day.
        rust = pkgs.rust-bin.selectLatestNightlyWith (toolchain: toolchain.default.override {
          extensions = [ "rust-src" "rust-analyzer" "rust-docs" "rustc-codegen-cranelift-preview" ];
          targets = [ "wasm32-unknown-unknown" ];
        });
        pre-commit-check = pre-commit-hooks.lib.${system}.run (v_flakes.files.preCommit { inherit pkgs; });
        pname = "ev_banking";

        rs = v_flakes.rs { inherit pkgs rust; };
        github = v_flakes.github {
          inherit pkgs pname rs;
          enable = true;
          lastSupportedVersion = "nightly-2026-05-12";
          gitignore.extra = ''
            ## Local Postgres
            .pg/
            ## Local TigerBeetle
            .tb/
            .tb-client
            ## Local Redis
            .redis/
            ## Node / Turborepo
            node_modules/
            .next/
            .turbo/
            ## Env
            .env
            .env.local
            ## LLMs
            AGENTS.md
            CLAUDE.md
            .claude/
            .pre-commit-config.yaml
          '';
          jobs = {
            warnings.augment = [ "tokei" "code-duplication" ];
            other.augment = [ "loc-badge" ];
          };
          lfs = true;
        };
        readme = v_flakes.readme-fw {
          inherit pkgs pname;
          repo = "EV-invest/banking";
          defaults = true;
          lastSupportedVersion = "nightly-1.92";
          rootDir = ./.;
          # crates_io (unpublished), loc (no gist) and ci (private repo) all
          # render "not found" — keep only the badges that resolve. The wasm
          # badge is injected in the devShell shellHook (no generator key for it).
          badges = [ "msrv" "docs_rs" ];
        };
        combined = v_flakes.utils.combine { inherit rust; modules = [ rs github readme ]; };

        # ── shared shims ────────────────────────────────────────────────────
        # rust-lld (wasm32 linker) embeds the wrong rpath on macOS — it looks for
        # libLLVM.dylib in bin/../lib/ but Nix puts it one level up in lib/.
        # The FALLBACK var only kicks in when normal resolution fails — exactly
        # rust-lld's case, never clang's (which would otherwise be forced onto
        # rustc's older libLLVM when linking host proc-macros).
        dyldFallback = ''export DYLD_FALLBACK_LIBRARY_PATH="${rust}/lib''${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"'';
        # tonic-build / prost-build shell out to protoc; point them at nixpkgs'.
        protocEnv = ''export PROTOC="${pkgs.protobuf}/bin/protoc"'';

        # ── TigerBeetle Rust client assets ──────────────────────────────────
        # Upstream's precompiled zig (fully static on linux, system-libs-only on
        # macOS) instead of nixpkgs zig, whose dynamic libLLVM turns the one-time
        # client build into a multi-GB download. The official tarball is ~50MB.
        zigBin =
          let
            dist = {
              x86_64-linux = { suffix = "x86_64-linux"; sha256 = "24aeeec8af16c381934a6cd7d95c807a8cb2cf7df9fa40d359aa884195c4716c"; };
              aarch64-linux = { suffix = "aarch64-linux"; sha256 = "f7a654acc967864f7a050ddacfaa778c7504a0eca8d2b678839c21eea47c992b"; };
              x86_64-darwin = { suffix = "x86_64-macos"; sha256 = "b0f8bdfb9035783db58dd6c19d7dea89892acc3814421853e5752fe4573e5f43"; };
              aarch64-darwin = { suffix = "aarch64-macos"; sha256 = "39f3dc5e79c22088ce878edc821dedb4ca5a1cd9f5ef915e9b3cc3053e8faefa"; };
            }.${system};
          in
          pkgs.stdenvNoCC.mkDerivation {
            pname = "zig-bin";
            version = "0.14.1";
            src = pkgs.fetchurl {
              url = "https://ziglang.org/download/0.14.1/zig-${dist.suffix}-0.14.1.tar.xz";
              inherit (dist) sha256;
            };
            dontConfigure = true;
            dontBuild = true;
            dontFixup = true;
            installPhase = ''
              mkdir -p $out/bin
              cp zig $out/bin/
              cp -r lib $out/lib
            '';
          };

        # Official precompiled server binary. nixpkgs' tigerbeetle lags behind and
        # a cluster evicts any client released after it (client_release_too_high) —
        # the server must be >= the 0.17.6 client built below. The release binaries
        # are static (zig-built), so they run on NixOS unpatched.
        tigerbeetleBin =
          let
            dist = {
              x86_64-linux = { file = "tigerbeetle-x86_64-linux.zip"; hash = "sha256-butV+rwsBnpLCCOV9KNzvCNCC8QbG/AR7ZRnl+Uyl7Y="; };
              aarch64-linux = { file = "tigerbeetle-aarch64-linux.zip"; hash = "sha256-JmsczIvW67WTrK0iCEDHcu9lhMyK84ZvhIs+lgL2bAs="; };
              x86_64-darwin = { file = "tigerbeetle-universal-macos.zip"; hash = "sha256-83nhQqHYu6PPKu4rH6rjD/J3hJinhXQ6b7C4hZ9//v8="; };
              aarch64-darwin = { file = "tigerbeetle-universal-macos.zip"; hash = "sha256-83nhQqHYu6PPKu4rH6rjD/J3hJinhXQ6b7C4hZ9//v8="; };
            }.${system};
          in
          pkgs.stdenvNoCC.mkDerivation {
            pname = "tigerbeetle-bin";
            version = "0.17.6";
            src = pkgs.fetchurl {
              url = "https://github.com/tigerbeetle/tigerbeetle/releases/download/0.17.6/${dist.file}";
              inherit (dist) hash;
            };
            nativeBuildInputs = [ pkgs.unzip ];
            unpackPhase = "unzip $src";
            dontConfigure = true;
            dontBuild = true;
            dontFixup = true;
            installPhase = ''
              mkdir -p $out/bin
              install -m755 tigerbeetle $out/bin/
            '';
          };

        # Builds the native C client library + header so the official tigerbeetle
        # Rust crate can link against them. The output is the src/clients/rust/
        # directory with compiled assets in place, ready as a Cargo path dep.
        tigerbeetleClient = pkgs.stdenv.mkDerivation {
          name = "tigerbeetle-client";
          src = pkgs.fetchzip {
            url = "https://github.com/tigerbeetle/tigerbeetle/archive/refs/tags/0.17.6.tar.gz";
            hash = "sha256-b519nsDbas+XOw3ulAnzpk2KwtJkeOC3e13urM2tUSM=";
          };
          # TigerBeetle pins its zig version exactly (0.17.6 → zig 0.14.1);
          # newer zig is rejected by build.zig with @compileError.
          nativeBuildInputs = [ zigBin pkgs.git ];
          # build.zig runs `git tag --merged HEAD^` at configure time and
          # unconditionally consumes 4+ version-shaped tags; the release tarball
          # has no .git, so fabricate a history with enough tags. They only feed
          # lazily-evaluated fetch_release steps that clients:rust never depends on.
          postPatch = ''
            git init -q
            git -c user.name=nix -c user.email=nix@localhost add -A
            git -c user.name=nix -c user.email=nix@localhost commit -qm base
            for v in 0.17.1 0.17.2 0.17.3 0.17.4 0.17.5; do git tag "$v"; done
            git -c user.name=nix -c user.email=nix@localhost commit -qm head --allow-empty
          '';
          buildPhase = ''
            export ZIG_GLOBAL_CACHE_DIR="$TMPDIR/zig-cache"
            zig build clients:rust -Drelease \
              -Dgit-commit=64899c7a41fd3d74c68da7bb2efcb7d208abd5f2 \
              -Dconfig-release=0.17.6 -Dconfig-release-client-min=0.17.6
          '';
          installPhase = ''
            mkdir -p $out
            cp -r src/clients/rust/* $out/
            printf '\n[workspace]\n' >> $out/Cargo.toml
          '';
        };

        # Symlink the TigerBeetle Rust client (with pre-built native assets) so
        # the path dependency in the workspace Cargo.toml resolves. Lives at the
        # repo root, NOT under a member dir: cargo's workspace exclude can never
        # match a path inside a member's directory.
        linkTbClient = ''
          tb_client_dir="$(git rev-parse --show-toplevel)/.tb-client"
          if [ ! -L "$tb_client_dir" ] || [ "$(readlink "$tb_client_dir")" != "${tigerbeetleClient}" ]; then
            rm -rf "$tb_client_dir"
            ln -s "${tigerbeetleClient}" "$tb_client_dir"
          fi
        '';

        # ── proto → OpenAPI → TS codegen plugin ─────────────────────────────
        # protoc-gen-connect-openapi as a pinned release binary (same approach as the
        # tigerbeetle/zig binaries above) — offline and supply-chain-pinned. It turns
        # the gRPC protos into an OpenAPI doc; @hey-api/openapi-ts then emits the
        # cabinet's TypeScript types. The proto stays the single wire source of truth.
        protocGenConnectOpenapi =
          let
            version = "0.25.7";
            dist = {
              x86_64-linux = { file = "protoc-gen-connect-openapi_${version}_linux_amd64.tar.gz"; hash = "sha256-3eqIe0Mt9Ucaxn72FncmtGpPM3rw7sT1+ImY+sX7bMI="; };
              aarch64-linux = { file = "protoc-gen-connect-openapi_${version}_linux_arm64.tar.gz"; hash = "sha256-1yKaOsZZdOAkIQUiyzkg52T930COJstQHrQUBuUU1uw="; };
              x86_64-darwin = { file = "protoc-gen-connect-openapi_${version}_darwin_all.tar.gz"; hash = "sha256-cXyGw8oDRkV7PkxfaVlrKLKy7GvwR0mAAcAr+tjln1I="; };
              aarch64-darwin = { file = "protoc-gen-connect-openapi_${version}_darwin_all.tar.gz"; hash = "sha256-cXyGw8oDRkV7PkxfaVlrKLKy7GvwR0mAAcAr+tjln1I="; };
            }.${system};
          in
          pkgs.stdenvNoCC.mkDerivation {
            pname = "protoc-gen-connect-openapi";
            inherit version;
            src = pkgs.fetchurl {
              url = "https://github.com/sudorandom/protoc-gen-connect-openapi/releases/download/v${version}/${dist.file}";
              inherit (dist) hash;
            };
            sourceRoot = ".";
            dontConfigure = true;
            dontBuild = true;
            dontFixup = true;
            installPhase = ''
              mkdir -p $out/bin
              install -m755 protoc-gen-connect-openapi $out/bin/
            '';
          };

        # ── piggybank (the hub server: core + auth, gRPC only) ──────────────
        # Runs `piggybank-core`, which spawns the core gRPC services and the auth
        # service as in-process tasks. A reachable Postgres + TigerBeetle are the
        # prerequisites (`.#db`/`.#tb`, or `.#dev`). No HTTP: browser traffic
        # reaches the hub through the `clients/cabinet` BFF. Defaults mirror
        # piggybank/core/.env.example; any value already in the environment wins.
        runPiggybank = pkgs.writeShellApplication {
          name = "run-piggybank";
          runtimeInputs = with pkgs; [ rust pkg-config openssl protobuf git ];
          text = ''
            ${dyldFallback}
            ${protocEnv}
            repo="$(git rev-parse --show-toplevel)"
            cd "$repo"

            ${linkTbClient}

            # Load the hub's local config (signing key, Google OAuth creds, …) the way
            # dotenvy would — but from piggybank/core/.env, since we run from the repo
            # root (dotenvy only checks ./.env). `set -a` exports each assignment;
            # already-set env vars are not overwritten by `.env`, and the defaults
            # below still fill anything the file leaves unset.
            set -a
            if [ -f piggybank/core/.env ]; then
              # shellcheck disable=SC1091
              . piggybank/core/.env
            fi
            set +a

            export DATABASE_URL="''${DATABASE_URL:-postgres://postgres@localhost:5432/ev_banking}"
            export GRPC_ADDR="''${GRPC_ADDR:-0.0.0.0:50051}"
            export AUTH_GRPC_ADDR="''${AUTH_GRPC_ADDR:-0.0.0.0:50052}"
            export RUST_LOG="''${RUST_LOG:-info,piggybank_core=debug,evbanking_auth=debug}"
            # Central-only refresh-token store; harmless if unused (auth is scaffold).
            export REDIS_URL="''${REDIS_URL:-redis://127.0.0.1:6379}"
            export TIGERBEETLE_ADDRESS="''${TIGERBEETLE_ADDRESS:-127.0.0.1:3033}"
            export TIGERBEETLE_CLUSTER_ID="''${TIGERBEETLE_CLUSTER_ID:-0}"
            export SIGNER_GRPC_ADDR="''${SIGNER_GRPC_ADDR:-http://127.0.0.1:50053}"
            exec cargo run -p piggybank-core
          '';
        };

        # ── signer (the separate-process key vault) ─────────────────────────
        # Runs `piggybank-signer`: generates chain keypairs, stores the private keys
        # encrypted at rest in its OWN database, and serves key provisioning over gRPC.
        # A reachable Postgres is the only prerequisite (`.#db`, or `.#dev`). Defaults
        # mirror piggybank/signer/.env.example; any value already set wins.
        runSigner = pkgs.writeShellApplication {
          name = "run-signer";
          runtimeInputs = with pkgs; [ rust pkg-config openssl protobuf git ];
          text = ''
            ${dyldFallback}
            ${protocEnv}
            repo="$(git rev-parse --show-toplevel)"
            cd "$repo"

            ${linkTbClient}

            set -a
            if [ -f piggybank/signer/.env ]; then
              # shellcheck disable=SC1091
              . piggybank/signer/.env
            fi
            set +a

            export SIGNER_DATABASE_URL="''${SIGNER_DATABASE_URL:-postgres://postgres@localhost:5432/ev_banking_signer}"
            export SIGNER_GRPC_ADDR="''${SIGNER_GRPC_ADDR:-0.0.0.0:50053}"
            # Dev-only KEK: ephemeral per boot when unset, so no key bytes live in the
            # repo. Production MUST inject a STABLE 32-byte KEK from a secrets store/KMS
            # (a rotating KEK can't open previously-sealed keys). Set WALLET_KEK in
            # piggybank/signer/.env to keep sealed keys openable across restarts.
            export WALLET_KEK="''${WALLET_KEK:-$(openssl rand -hex 32)}"
            export RUST_LOG="''${RUST_LOG:-info,piggybank_signer=debug}"
            exec cargo run -p piggybank-signer
          '';
        };

        # ── cabinet backend (the BFF) ───────────────────────────────────────
        # Runs `cabinet-backend`: the cabinet's stateless HTTP BFF. It holds the session
        # + runs the OAuth flow and proxies the browser's /api/* to the piggybank money
        # plane (:50051) and the concierge identity plane (:50061 — run from the sibling
        # `concierge` repo). `linkTbClient` is required only so the Cargo workspace (which
        # contains the TB-using piggybank crates) resolves; the BFF itself never uses TB.
        # Defaults mirror clients/cabinet/backend/.env.example; any value already set wins.
        runCabinetBackend = pkgs.writeShellApplication {
          name = "run-cabinet-backend";
          runtimeInputs = with pkgs; [ rust pkg-config openssl protobuf git ];
          text = ''
            ${dyldFallback}
            ${protocEnv}
            repo="$(git rev-parse --show-toplevel)"
            cd "$repo"

            ${linkTbClient}

            set -a
            if [ -f clients/cabinet/backend/.env ]; then
              # shellcheck disable=SC1091
              . clients/cabinet/backend/.env
            fi
            set +a

            export CABINET_BACKEND_BIND="''${CABINET_BACKEND_BIND:-0.0.0.0:4000}"
            export PIGGYBANK_GRPC_ADDR="''${PIGGYBANK_GRPC_ADDR:-http://127.0.0.1:50051}"
            export CONCIERGE_GRPC_ADDR="''${CONCIERGE_GRPC_ADDR:-http://127.0.0.1:50061}"
            export RUST_LOG="''${RUST_LOG:-info,cabinet_backend=debug}"
            exec cargo run -p cabinet-backend
          '';
        };

        # ── clients (Turborepo: Next.js host) ───────────────────────────────
        # npm workspaces rooted at the repo; deps install once into the hoisted
        # node_modules (`npm install` also generates/updates the lockfile). The `cabinet`
        # frontend proxies /api/* to the cabinet backend (BFF) over HTTP (same-origin).
        runCabinet = pkgs.writeShellApplication {
          name = "run-cabinet";
          runtimeInputs = with pkgs; [ nodejs git ];
          text = ''
            repo="$(git rev-parse --show-toplevel)"
            cd "$repo"
            [ -d node_modules/next ] || npm install
            export CABINET_BACKEND_URL="''${CABINET_BACKEND_URL:-http://127.0.0.1:4000}"
            exec npm run dev --workspace @evbanking/cabinet
          '';
        };

        # ── contracts codegen (proto → OpenAPI → cabinet TS types) ──────────
        # `nix run .#gen-api` regenerates `contracts/openapi.json` from the protos and
        # the cabinet's `shared/contracts/gen` types from that. Run it after editing a
        # proto; the outputs are committed so the app builds without the toolchain.
        #
        # The cabinet's identity surface (profile + sessions) is served by the concierge
        # plane, so its TS types come from concierge's OWN proto — not banking's copy —
        # sourced from the pinned `evconcierge_contracts` git dep that the BFF already
        # compiles against (resolved via `cargo metadata`, so it tracks Cargo.lock's pin).
        # Both protos feed one merged OpenAPI doc; service FQNs keep paths/schemas
        # namespaced (`banking.v1.*` vs `concierge.v1.*`), so the gen emits both
        # `BankingV1*` and `ConciergeV1*` types with no collision.
        runGenApi = pkgs.writeShellApplication {
          name = "run-gen-api";
          runtimeInputs = with pkgs; [ protobuf protocGenConnectOpenapi nodejs git cargo jq ];
          text = ''
            repo="$(git rev-parse --show-toplevel)"
            cd "$repo"
            cc_dir="$(dirname "$(cargo metadata --format-version 1 --manifest-path contracts/Cargo.toml \
              | jq -r '.packages[] | select(.name=="evconcierge_contracts") | .manifest_path')")"
            echo "▶ proto (banking + concierge identity) → contracts/openapi.json"
            protoc -I contracts/proto -I "$cc_dir/proto" \
              --connect-openapi_out=contracts \
              --connect-openapi_opt=format=json,path=openapi.json,with-proto-names \
              contracts/proto/banking/v1/*.proto \
              "$cc_dir/proto/concierge/v1/directory.proto" \
              "$cc_dir/proto/concierge/v1/auth.proto"
            echo "▶ openapi.json → cabinet TypeScript types"
            [ -d node_modules ] || npm install
            npm run gen:api --workspace @evbanking/cabinet
            echo "✓ regenerated contracts/openapi.json + clients/cabinet/shared/contracts/gen"
          '';
        };

        # ── cross-repo concierge pin guard ──────────────────────────────────
        # `nix run .#concierge-pin-check` — assert the pinned `evconcierge_contracts`
        # rev (the identity wire contract banking compiles and re-aliases its cabinet
        # TS from) is an ancestor of concierge origin/main with matching proto bytes.
        # CI entry point for the contract-parity guard; needs network to the remote.
        runConciergePinCheck = pkgs.writeShellApplication {
          name = "run-concierge-pin-check";
          runtimeInputs = with pkgs; [ git gnused coreutils gnugrep ];
          text = ''exec bash "$(git rev-parse --show-toplevel)/contracts/concierge-pin-check.sh"'';
        };

        # ── local Redis ─────────────────────────────────────────────────────
        # The CENTRAL auth refresh-token store only — never a per-service cache.
        # Ephemeral dev instance under .redis/ (gitignored).
        runRedis = pkgs.writeShellApplication {
          name = "run-redis";
          runtimeInputs = with pkgs; [ redis git ];
          text = ''
            repo="$(git rev-parse --show-toplevel)"
            mkdir -p "$repo/.redis"
            echo "Redis ready on 127.0.0.1:''${REDIS_PORT:-6379}"
            exec redis-server --port "''${REDIS_PORT:-6379}" --dir "$repo/.redis" --save "" --appendonly no
          '';
        };

        # ── local Postgres ──────────────────────────────────────────────────
        # Project-local dev database under .pg/ (gitignored). First run initdb's a
        # trust-auth cluster and creates the databases (`ev_banking` for the hub and
        # `ev_banking_signer` for the signer's wallet_secrets); later runs just start it.
        runPostgres = pkgs.writeShellApplication {
          name = "run-postgres";
          runtimeInputs = with pkgs; [ postgresql git coreutils gnugrep ];
          text = ''
            repo="$(git rev-parse --show-toplevel)"
            export PGDATA="$repo/.pg/data"
            sockets="$repo/.pg/sockets"
            port="''${PGPORT:-5432}"
            dbs="''${PGDATABASES:-ev_banking ev_banking_signer}"

            mkdir -p "$sockets"
            if [ ! -s "$PGDATA/PG_VERSION" ]; then
              echo "initialising postgres cluster in $PGDATA"
              initdb --username=postgres --auth=trust --pgdata="$PGDATA" >/dev/null
            fi
            chmod 0700 "$PGDATA"

            (
              until pg_isready --host="$sockets" --port="$port" --quiet; do sleep 0.2; done
              for db in $dbs; do
                if ! psql --host="$sockets" --port="$port" --username=postgres --dbname=postgres \
                       --tuples-only --no-align \
                       --command "SELECT 1 FROM pg_database WHERE datname='$db'" | grep -q 1; then
                  createdb --host="$sockets" --port="$port" --username=postgres "$db"
                  echo "created database '$db'"
                fi
              done
              echo "postgres ready on 127.0.0.1:$port (databases: $dbs, user 'postgres', trust auth)"
            ) &

            exec postgres -D "$PGDATA" -k "$sockets" -h 127.0.0.1 -p "$port"
          '';
        };

        # ── local TigerBeetle ───────────────────────────────────────────────
        # Project-local ledger under .tb/ (gitignored). First run formats a
        # single-replica cluster; later runs just start it. Port 3033 keeps the
        # ledger off the 3000 web range owned by `cabinet`.
        runTigerbeetle = pkgs.writeShellApplication {
          name = "run-tigerbeetle";
          runtimeInputs = [ tigerbeetleBin pkgs.git ];
          text = ''
            repo="$(git rev-parse --show-toplevel)"
            export TB_DATA="$repo/.tb/data"
            port="''${TBPORT:-3033}"
            cluster_id="''${TBCLUSTER:-0}"
            data_file="$TB_DATA/''${cluster_id}_0.tigerbeetle"

            mkdir -p "$TB_DATA"
            if [ ! -f "$data_file" ]; then
              echo "formatting TigerBeetle data file (cluster=''${cluster_id}, replica=0, replica-count=1)"
              tigerbeetle format --cluster="$cluster_id" --replica=0 --replica-count=1 "$data_file"
            fi

            echo "TigerBeetle ready on 127.0.0.1:$port (cluster ''${cluster_id})"
            exec tigerbeetle start --addresses="127.0.0.1:$port" "$data_file"
          '';
        };

        # ── full dev orchestrator ───────────────────────────────────────────
        # `nix run .#dev` → Postgres + TigerBeetle + Redis + signer + piggybank +
        # cabinet-backend + cabinet. Postgres starts first, then the rest. A single trap
        # tears the whole tree down on exit. (Concierge, the identity plane, lives in its
        # own repo — start it there for the auth/profile/session flows.)
        runDev = pkgs.writeShellApplication {
          name = "run-dev";
          runtimeInputs = with pkgs; [ postgresql git coreutils ];
          text = ''
            pids=()
            cleanup() {
              echo; echo "shutting down dev stack…"
              [ ''${#pids[@]} -gt 0 ] && kill "''${pids[@]}" 2>/dev/null || true
              wait 2>/dev/null || true
            }
            trap cleanup EXIT INT TERM

            echo "▶ postgres"
            ${runPostgres}/bin/run-postgres & pids+=($!)

            echo "  waiting for postgres on 127.0.0.1:''${PGPORT:-5432}…"
            until pg_isready --host=127.0.0.1 --port="''${PGPORT:-5432}" --quiet; do sleep 0.3; done

            echo "▶ tigerbeetle"
            ${runTigerbeetle}/bin/run-tigerbeetle & pids+=($!)
            echo "▶ redis"
            ${runRedis}/bin/run-redis & pids+=($!)

            echo "▶ signer    (:50053)"
            ${runSigner}/bin/run-signer & pids+=($!)
            echo "▶ piggybank (:50051 core / :50052 auth)"
            ${runPiggybank}/bin/run-piggybank & pids+=($!)
            echo "▶ cabinet-backend (:4000, BFF)"
            ${runCabinetBackend}/bin/run-cabinet-backend & pids+=($!)
            echo "▶ cabinet   (:3000)"
            ${runCabinet}/bin/run-cabinet & pids+=($!)

            wait
          '';
        };
      in
      {
        # `nix run .#dev`            → everything (postgres + tigerbeetle + redis + signer + piggybank + cabinet-backend + cabinet)
        # `nix run .#piggybank`      → hub server: core gRPC + auth tasks (applies DB migrations on boot; needs DB + TB + signer: `.#db`/`.#tb`/`.#signer`, or `.#dev`)
        # `nix run .#signer`         → key vault: generates+seals chain keys (applies its own DB migrations on boot; needs DB: `.#db`, or `.#dev`)
        # `nix run .#cabinet-backend`→ cabinet BFF (:4000; needs piggybank on :50051; identity flows need concierge on :50061 from its own repo)
        # `nix run .#cabinet`        → Next.js host shell (:3000, proxies /api/* to the cabinet backend on :4000)
        # `nix run .#db`             → local Postgres only (creates ev_banking + ev_banking_signer)
        # `nix run .#tb`        → local TigerBeetle only
        # `nix run .#redis`     → local Redis (central auth store) only
        # `nix run .#gen-api`   → regenerate contracts/openapi.json + cabinet TS types from the proto
        # `nix run .#concierge-pin-check` → assert the concierge contract pin is an ancestor of origin/main + bytes match
        # Author new migrations with the sqlx CLI (in the dev shell):
        #   sqlx migrate add --source piggybank/core/migrations --sequential <name>
        apps = {
          dev = { type = "app"; program = "${runDev}/bin/run-dev"; };
          piggybank = { type = "app"; program = "${runPiggybank}/bin/run-piggybank"; };
          signer = { type = "app"; program = "${runSigner}/bin/run-signer"; };
          cabinet-backend = { type = "app"; program = "${runCabinetBackend}/bin/run-cabinet-backend"; };
          cabinet = { type = "app"; program = "${runCabinet}/bin/run-cabinet"; };
          db = { type = "app"; program = "${runPostgres}/bin/run-postgres"; };
          tb = { type = "app"; program = "${runTigerbeetle}/bin/run-tigerbeetle"; };
          redis = { type = "app"; program = "${runRedis}/bin/run-redis"; };
          gen-api = { type = "app"; program = "${runGenApi}/bin/run-gen-api"; };
          concierge-pin-check = { type = "app"; program = "${runConciergePinCheck}/bin/run-concierge-pin-check"; };
        };

        devShells.default =
          with pkgs;
          mkShell {
            shellHook =
              pre-commit-check.shellHook
              + combined.shellHook
              + ''
                cp -f ${(v_flakes.files.treefmt) { inherit pkgs; }} ./.treefmt.toml

                export DYLD_FALLBACK_LIBRARY_PATH="${rust}/lib''${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"
                export PROTOC="${pkgs.protobuf}/bin/protoc"

                # readme-fw regenerates README on every entry and has no wasm badge
                # key — append a WebAssembly badge after the docs.rs one, once.
                if [ -f ./README.md ] && ! grep -qi WebAssembly ./README.md; then
                  ${gnused}/bin/sed -i '/docs\.rs/a [<img alt="WebAssembly" src="https://img.shields.io/badge/WebAssembly-654FF0?logo=webassembly&logoColor=white" height="20">](https://webassembly.org)' ./README.md
                fi

                ${linkTbClient}
              '';

            packages = [
              nodejs
              redis
              openssl
              pkg-config
              protobuf
              clang-tools
              rust
              mold
              postgresql
              sqlx-cli
              tigerbeetleBin
              protocGenConnectOpenapi
              sccache
            ] ++ pre-commit-check.enabledPackages ++ combined.enabledPackages;

            env.RUST_BACKTRACE = 1;
            env.RUST_LIB_BACKTRACE = 0;
            # shared compile cache across builds; incremental off (sccache requires it)
            env.RUSTC_WRAPPER = "sccache";
            env.CARGO_INCREMENTAL = "0";
          };
      }
    );
}
