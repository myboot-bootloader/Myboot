{
  description = "MyBoot — a transactional cross-OS UEFI boot manager (Rust + asm)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        # Stable Rust with BOTH the host target (for host tests / host-cli / xtask)
        # and the UEFI target (for the loader itself).
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          targets = [ "x86_64-unknown-uefi" ];
          extensions = [ "rust-src" "clippy" "rustfmt" ];
        };

        # buildRustPackage, but using the SAME rustc/cargo as the dev shell above
        # (the one with the x86_64-unknown-uefi target already installed) instead of
        # whatever nixpkgs' own channel ships.
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        # --- the UEFI loader (bin/myboot, built from the workspace root) ---
        # NOTE: this repo gitignores Cargo.lock. Run `cargo generate-lockfile` at
        # the repo root once, and commit it (or drop it from .gitignore), before
        # `nix build` will see ./Cargo.lock.
        myboot-efi = rustPlatform.buildRustPackage {
          pname = "myboot-efi";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          # Only the UEFI binary + its path deps (engine, platform), not the whole
          # workspace — and no_std/cross means the normal test harness can't run
          # in the sandbox anyway.
          buildPhase = ''
            runHook preBuild
            cargo build -p myboot --release --target x86_64-unknown-uefi --offline
            runHook postBuild
          '';
          doCheck = false;
          installPhase = ''
            runHook preInstall
            mkdir -p $out/lib/myboot
            cp target/x86_64-unknown-uefi/release/myboot.efi $out/lib/myboot/myboot.efi
            runHook postInstall
          '';

          meta.description = "MyBoot UEFI boot manager (myboot.efi)";
        };

        # --- the host-side CLI (host-cli/, deliberately excluded from the
        # no_std workspace — see the root Cargo.toml's `exclude`) ---
        # Same Cargo.lock caveat: `cd host-cli && cargo generate-lockfile` once.
        myboot-host = rustPlatform.buildRustPackage {
          pname = "myboot-host";
          version = "0.1.0";
          src = ./host-cli;
          cargoLock.lockFile = ./host-cli/Cargo.lock;
          meta.description = "MyBoot host-side management CLI";
          # produces $out/bin/myboot — see host-cli/Cargo.toml's [[bin]] name
        };

        # --- the `boot.loader.external.installHook` entry point ---
        # Deliberately as thin as `myboot install` itself: it only copies the
        # loader + writes config.toml on the ESP (both idempotent — `myboot
        # install` never overwrites an existing config), and does NOT touch
        # NVRAM. That mirrors host-cli's own stated design ("prints the exact
        # efibootmgr command ... rather than writing NVRAM itself, so the user
        # stays in control") — register the firmware entry by hand, once, with
        # the efibootmgr command `myboot install` prints, the same way §11.4
        # of TESTING_AND_INSTALL.md does it manually. That also means repeated
        # `nixos-rebuild switch` runs can't pile up duplicate NVRAM entries.
        myboot-install = pkgs.writeShellApplication {
          name = "myboot-install";
          runtimeInputs = [ myboot-host ];
          text = ''
            ESP="''${MYBOOT_ESP:-/boot}"
            myboot install --esp "$ESP" --efi "${myboot-efi}/lib/myboot/myboot.efi"
          '';
        };

        myboot = pkgs.symlinkJoin {
          name = "myboot";
          paths = [ myboot-efi myboot-host myboot-install ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          packages = [
            rustToolchain
            pkgs.qemu
            pkgs.OVMF
            pkgs.edk2-uefi-shell   # a real EFI app to chainload in QEMU tests
            pkgs.python3
            pkgs.python3Packages.pexpect  # drives QEMU in the pyboot test harness
            pkgs.efibootmgr
            pkgs.xorriso
            pkgs.mtools
            pkgs.dosfstools
            pkgs.parted
            pkgs.gptfdisk
            pkgs.util-linux

            # website/ (Next.js + TiDB) local dev tooling
            pkgs.bun
            pkgs.openssl       # generate BETTER_AUTH_SECRET / IP_HASH_SALT
            pkgs.mariadb       # mysql/mariadb client, for applying SQL directly to TiDB
          ];

          shellHook = ''
            echo "MyBoot dev shell — tooling: ./manage.py --help"
            echo "  $(rustc --version)"
            echo "  $(qemu-system-x86_64 --version | head -n1)"

            mkdir -p ./firmware
            if [ ! -f ./firmware/OVMF_VARS.fd ]; then
              cp ${pkgs.OVMF.fd}/FV/OVMF_VARS.fd ./firmware/OVMF_VARS.fd
              chmod +w ./firmware/OVMF_VARS.fd
            fi
            export OVMF_CODE="${pkgs.OVMF.fd}/FV/OVMF_CODE.fd"
            export OVMF_VARS="$(pwd)/firmware/OVMF_VARS.fd"
            # A real, different EFI application for `./manage.py smoke` to chainload as a
            # true end-to-end boot test (the UEFI shell appears on handoff).
            for cand in ${pkgs.edk2-uefi-shell}/shell.efi ${pkgs.edk2-uefi-shell}/*.efi; do
              [ -f "$cand" ] && export MYBOOT_UEFI_SHELL="$cand" && break
            done
            echo "  OVMF_CODE=$OVMF_CODE"
            echo "  OVMF_VARS=$OVMF_VARS"
            [ -n "''${MYBOOT_UEFI_SHELL:-}" ] && echo "  MYBOOT_UEFI_SHELL=$MYBOOT_UEFI_SHELL"
            echo ""
            echo "  cargo test  (per pure crate)          → ./scripts/run-tests.sh"
            echo "  build the loader for UEFI             → cargo build -p myboot --release --target x86_64-unknown-uefi"
            echo "  run in QEMU with a synthetic ESP      → ./manage.py smoke"
          '';
        };

        packages = {
          inherit myboot myboot-efi myboot-host myboot-install;
          default = myboot;
        };
      });
}
