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
        pname = "ev_fund";

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
            ## Node
            node_modules/
            ## Generated frontend CSS
            frontend/public/tailwind.css
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
          lfs = false;
        };
        readme = v_flakes.readme-fw {
          inherit pkgs pname;
          repo = "EV-invest/fund";
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
        # repo root, NOT under backend/: cargo's workspace exclude can never match
        # a path inside a member's directory.
        linkTbClient = ''
          tb_client_dir="$(git rev-parse --show-toplevel)/.tb-client"
          if [ ! -L "$tb_client_dir" ] || [ "$(readlink "$tb_client_dir")" != "${tigerbeetleClient}" ]; then
            rm -rf "$tb_client_dir"
            ln -s "${tigerbeetleClient}" "$tb_client_dir"
          fi
        '';

        # ── backend (Axum HTTP + tonic gRPC) ────────────────────────────────
        # Migrations run automatically on startup, so a reachable Postgres is the
        # only prerequisite (`.#db`, or `.#dev` which boots one first). Defaults
        # mirror backend/.env.example; any value already in the environment wins.
        runBackend = pkgs.writeShellApplication {
          name = "run-backend";
          runtimeInputs = with pkgs; [ rust pkg-config openssl protobuf git ];
          text = ''
            ${dyldFallback}
            ${protocEnv}
            repo="$(git rev-parse --show-toplevel)"
            cd "$repo"

            ${linkTbClient}

            export DATABASE_URL="''${DATABASE_URL:-postgres://postgres@localhost:5432/ev_fund}"
            export BIND_ADDR="''${BIND_ADDR:-0.0.0.0:8080}"
            export GRPC_ADDR="''${GRPC_ADDR:-0.0.0.0:50051}"
            export RUST_LOG="''${RUST_LOG:-info,backend=debug}"
            export TIGERBEETLE_ADDRESS="''${TIGERBEETLE_ADDRESS:-127.0.0.1:3001}"
            export TIGERBEETLE_CLUSTER_ID="''${TIGERBEETLE_CLUSTER_ID:-0}"
            exec cargo run -p backend
          '';
        };

        # ── frontend (Dioxus / WASM) ────────────────────────────────────────
        # Build Tailwind once, keep it rebuilding in the background (the `@source`
        # scan in frontend/input.css picks up class names from RSX), then serve.
        # dx defaults to :8080 like the backend, so pin frontend to :3000.
        runFrontend = pkgs.writeShellApplication {
          name = "run-frontend";
          runtimeInputs = with pkgs; [ rust dioxus-cli nodejs git ];
          text = ''
            ${dyldFallback}
            repo="$(git rev-parse --show-toplevel)"
            cd "$repo"
            # tailwind v4 resolves `@import "tailwindcss"` from node_modules, so
            # the package must be installed before the CSS build.
            if [ ! -d node_modules/tailwindcss ] || [ package-lock.json -nt node_modules/.package-lock.json ]; then
              npm install
            fi
            npm run css
            npm run css:watch & css=$!
            trap 'kill "$css" 2>/dev/null || true' EXIT INT TERM
            # `--interactive false`: dx's default full-screen TUI gets corrupted
            # when it shares stdout with the css watcher or the `.#dev` processes.
            exec dx serve --package frontend --port "''${FRONTEND_PORT:-3000}" --interactive false
          '';
        };

        # ── local Postgres ──────────────────────────────────────────────────
        # Project-local dev database under .pg/ (gitignored). First run initdb's a
        # trust-auth cluster and creates `ev_fund`; later runs just start it.
        runPostgres = pkgs.writeShellApplication {
          name = "run-postgres";
          runtimeInputs = with pkgs; [ postgresql git coreutils gnugrep ];
          text = ''
            repo="$(git rev-parse --show-toplevel)"
            export PGDATA="$repo/.pg/data"
            sockets="$repo/.pg/sockets"
            port="''${PGPORT:-5432}"
            db="''${PGDATABASE:-ev_fund}"

            mkdir -p "$sockets"
            if [ ! -s "$PGDATA/PG_VERSION" ]; then
              echo "initialising postgres cluster in $PGDATA"
              initdb --username=postgres --auth=trust --pgdata="$PGDATA" >/dev/null
            fi
            chmod 0700 "$PGDATA"

            (
              until pg_isready --host="$sockets" --port="$port" --quiet; do sleep 0.2; done
              if ! psql --host="$sockets" --port="$port" --username=postgres --dbname=postgres \
                     --tuples-only --no-align \
                     --command "SELECT 1 FROM pg_database WHERE datname='$db'" | grep -q 1; then
                createdb --host="$sockets" --port="$port" --username=postgres "$db"
                echo "created database '$db'"
              fi
              echo "postgres ready on 127.0.0.1:$port (db '$db', user 'postgres', trust auth)"
            ) &

            exec postgres -D "$PGDATA" -k "$sockets" -h 127.0.0.1 -p "$port"
          '';
        };

        # ── local TigerBeetle ───────────────────────────────────────────────
        # Project-local ledger under .tb/ (gitignored). First run formats a
        # single-replica cluster; later runs just start it.
        runTigerbeetle = pkgs.writeShellApplication {
          name = "run-tigerbeetle";
          runtimeInputs = [ tigerbeetleBin pkgs.git ];
          text = ''
            repo="$(git rev-parse --show-toplevel)"
            export TB_DATA="$repo/.tb/data"
            port="''${TBPORT:-3001}"
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
        # `nix run .#dev` → Postgres + TigerBeetle + backend + frontend, together.
        # Postgres starts first, then TigerBeetle; backend launches once both
        # accept connections. A single trap tears the whole tree down on exit.
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

            echo "▶ backend   (:8080 http / :50051 grpc)"
            ${runBackend}/bin/run-backend & pids+=($!)
            echo "▶ frontend  (:3000)"
            ${runFrontend}/bin/run-frontend & pids+=($!)

            wait
          '';
        };
      in
      {
        # `nix run .#dev`      → everything (postgres + tigerbeetle + backend + frontend)
        # `nix run .#backend`  → Axum + tonic API only (needs a DB: `.#db` or `.#dev`)
        # `nix run .#frontend` → Dioxus app + Tailwind watch only
        # `nix run .#db`       → local Postgres only
        # `nix run .#tb`       → local TigerBeetle only
        apps = {
          dev = { type = "app"; program = "${runDev}/bin/run-dev"; };
          backend = { type = "app"; program = "${runBackend}/bin/run-backend"; };
          frontend = { type = "app"; program = "${runFrontend}/bin/run-frontend"; };
          db = { type = "app"; program = "${runPostgres}/bin/run-postgres"; };
          tb = { type = "app"; program = "${runTigerbeetle}/bin/run-tigerbeetle"; };
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
              openssl
              pkg-config
              protobuf
              clang-tools
              rust
              mold
              postgresql
              tigerbeetleBin
            ] ++ pre-commit-check.enabledPackages ++ combined.enabledPackages;

            env.RUST_BACKTRACE = 1;
            env.RUST_LIB_BACKTRACE = 0;
          };
      }
    );
}
