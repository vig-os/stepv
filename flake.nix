{
  description = "Project development environment (vigOS toolchain).";

  # Downstream repos consume the shared toolchain as a flake INPUT, so updating
  # the dev environment means bumping that input — it never overwrites your
  # files. To update: `nix flake update vigos`.
  inputs = {
    # The shared vigOS toolchain (single source of truth).
    # This scaffold deliberately FLOATS on the default branch so a fresh
    # project works before its first pin. Once you depend on stability
    # (especially the vigos.* home-manager module options), pin a release
    # tag instead and bump deliberately:
    #   vigos.url = "github:vig-os/devkit?ref=<tag>";
    # Policy: https://github.com/vig-os/devkit/blob/main/docs/NIX.md
    # "Home-manager modules - versioning & release policy".
    vigos.url = "github:vig-os/devkit";
    # Follow vigos's pinned nixpkgs + flake-utils so your tools match the
    # toolchain exactly (one resolved nixpkgs, no drift).
    nixpkgs.follows = "vigos/nixpkgs";
    flake-utils.follows = "vigos/flake-utils";
  };

  outputs =
    {
      self,
      vigos,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ vigos.overlays.default ];
          config.allowUnfree = true;
        };

        # ────────────────────────────────────────────────────────────────────
        # Your project tools go here. This block is YOURS: a dev-environment
        # update never overwrites it (scaffold-once / never-overwrite, the same
        # guarantee as justfile.project and docker-compose.project.yaml).
        #
        #   extraPackages = pkgs: [
        #     pkgs.postgresql_16
        #     pkgs.ffmpeg
        #   ];
        # ────────────────────────────────────────────────────────────────────
        extraPackages = pkgs: [
          # Fixture corpus fetching for the robustness harness (plan.md §5).
          pkgs.curl
          pkgs.unzip
          # Plan B kernel (plan.md §3): native OCCT. The occt-wasm spike failed
          # its gate — see plan.md §5 "S1 result". devkit's `native` module
          # (added to `modules` below) brings cc/c++/cmake/make/pkg-config but
          # deliberately leaves third-party libraries to extraPackages, so OCCT
          # itself must be named here.
          pkgs.opencascade-occt
        ];

        # Devkit knobs read from .vig-os (#1224, #1432, #1431, #1282, #1633): the
        # flake-generated pre-commit hooks — the branch guard and the
        # commit-message validator — follow the workspace manifest, mirroring
        # the scaffolded .pre-commit-config.yaml renders (#1434). Managed
        # block; leave it.
        vigOsValue =
          key:
          let
            vigOsPath = self + "/.vig-os";
            declared = builtins.filter (l: nixpkgs.lib.hasPrefix "${key}=" l) (
              nixpkgs.lib.splitString "\n" (builtins.readFile vigOsPath)
            );
          in
          if !builtins.pathExists vigOsPath || declared == [ ] then
            ""
          else
            nixpkgs.lib.removePrefix "${key}=" (builtins.head declared);

        # A comma-separated manifest list -> a Nix list, or null when the key
        # is absent/blank (= "keep the devkit default"). Whitespace around
        # entries is trimmed and empty entries dropped, matching how
        # init-workspace.sh resolves the same keys; validation (charset,
        # non-empty) lives in mkProjectShell, which fails eval loudly on a bad
        # value.
        # deadnix: skip -- unused only because mkRustProject does not forward it (plan.md §7)
        vigOsList =
          key:
          let
            entries = builtins.filter (t: t != "") (
              map (t: nixpkgs.lib.trim t) (nixpkgs.lib.splitString "," (vigOsValue key))
            );
          in
          if entries == [ ] then null else entries;

        # Workflow model (#1224): a `trunk` workspace drops the dev-branch
        # clause. `gitflow` (the default) and an absent/blank value are inert.
        workflow = if vigOsValue "DEVKIT_WORKFLOW" == "trunk" then "trunk" else "gitflow";

        # Branch-type set (#1432): DEVKIT_BRANCH_TYPES replaces the
        # issue-numbered alternation of the branch guard.
        # deadnix: skip -- unused only because mkRustProject does not forward it (plan.md §7)
        branchTypes = vigOsList "DEVKIT_BRANCH_TYPES";

        # Approved commit types (#1431): DEVKIT_COMMIT_TYPES replaces the
        # validate-commit-msg `--types` list, so the local hook agrees with
        # CI's validate-commit-range (#1434).
        # deadnix: skip -- unused only because mkRustProject does not forward it (plan.md §7)
        commitTypes = vigOsList "DEVKIT_COMMIT_TYPES";

        # Refs policy (#1282): DEVKIT_REFS_POLICY steers whether a commit needs
        # a `Refs: #N` line — chore-optional (default) | optional | required.
        # Absent/blank forwards null (= the default); an unknown literal fails
        # eval loudly in mkProjectShell (#1434).
        # deadnix: skip -- unused only because mkRustProject does not forward it (plan.md §7)
        refsPolicy =
          let
            raw = nixpkgs.lib.trim (vigOsValue "DEVKIT_REFS_POLICY");
          in
          if raw == "" then null else raw;

        # Refs-optional types (#1633): DEVKIT_REFS_OPTIONAL_TYPES names the
        # commit types that may omit `Refs:` and WINS over DEVKIT_REFS_POLICY.
        # Absent/blank forwards null (= the policy decides); a value outside
        # the approved types fails eval loudly in mkProjectShell.
        # deadnix: skip -- unused only because mkRustProject does not forward it (plan.md §7)
        refsOptionalTypes = vigOsList "DEVKIT_REFS_OPTIONAL_TYPES";

        # ────────────────────────────────────────────────────────────────────
        # Rust language pack (devkit #1400 / #1427).
        #
        # ONE call gives the dev shell, `checks` (fmt, clippy, nextest,
        # doctests, cargo-doc) and `packages`. It is not optional sugar: the
        # `rust` capability module REFUSES to load bare — `modules = [ "rust" ]`
        # would hand you a toolchain, a green `nix flake check` that builds
        # nothing, and CI that compiles nothing, so devkit made that an
        # eval-time error and `mkRustProject` the only supported path.
        #
        # KNOWN GAP (raise with devkit before relying on it): mkRustProject's
        # argument list has no `branchTypes` / `commitTypes` / `refsPolicy` /
        # `refsOptionalTypes`, so those .vig-os knobs do NOT reach the
        # flake-generated hooks the way they do through mkProjectShell. Inert
        # here — all four keys are empty in .vig-os, so every one resolves to
        # its devkit default. Set one and it will be silently ignored.
        # ────────────────────────────────────────────────────────────────────
        # The Plan B kernel (plan.md §3): kernel/stepv-occt, native OCCT. Its
        # own derivation, so the Rust checks below run the CLI tests against a
        # REAL kernel (they fail rather than skip without one), and so the
        # product package can ship it in libexec/stepv beside the CLI.
        kernel = pkgs.stdenv.mkDerivation {
          pname = "stepv-occt";
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
          src = ./kernel;
          nativeBuildInputs = [ pkgs.cmake ];
          buildInputs = [ pkgs.opencascade-occt ];
          cmakeFlags = [ "-DCMAKE_BUILD_TYPE=Release" ];
          meta.description = "stepv's OCCT kernel: STEP/IGES/BREP to meshes, run as a subprocess";
        };

        rust = vigos.lib.mkRustProject {
          inherit pkgs workflow;
          src = ./.;

          # fenix needs the content hash of the toolchain rust-toolchain.toml
          # resolves to. Shared with gerchowl/cxad, which pins the same channel
          # and components byte-for-byte. Re-derive on a channel bump:
          #   nix build .#devShells.${system}.default 2>&1 | grep 'got:'
          toolchainHash = "sha256-mvUGEOHYJpn3ikC5hckneuGixaC+yGrkMM/liDIDgoU=";

          # Named rather than null so a break is attributed to `stepv` instead
          # of reported against "the workspace" (plan.md §"Crate layout").
          crates = [
            "stepv"
            "stepv-capi"
          ];

          # The harness corpus manifest and the committed CLI-test corpus live
          # outside the cargo source filter; without this, crane drops them.
          extraSrcFiles = [
            "tests/fixtures"
            "tests/data"
          ];

          # Every crane derivation (nextest included) sees the built kernel.
          craneArgs.STEPV_OCCT = "${kernel}/libexec/stepv/stepv-occt";

          # Host-runner hooks (#1167): direnv CI runs on the bare host runner,
          # so the flake GENERATES .pre-commit-config.yaml from the shared base
          # hook set, resolved from the Nix store. Customize here.
          hooks = { };
          # Generated CAD test data (kernel/fixture-gen): byte-for-byte what
          # OCCT writes, so the whitespace fixers must not "correct" it, or
          # every `just test-data` would re-dirty the tree.
          # The licence texts in licenses/ are verbatim upstream copies: a
          # whitespace "fix" would make them no longer the licence.
          hooksExcludes = [
            "^tests/data/"
            "^licenses/"
          ];

          tools = [
            "nextest"
            "deny"
            "shear"
            "about"
          ];

          extraPackages = extraPackages pkgs;

          # cc/c++/cmake/make/pkg-config for the Plan B C++ kernel (kernel/).
          modules = [ "native" ];
        };
      in
      {
        devShells.default = rust.devShell;

        # fmt / clippy / nextest / doctest / cargo-doc per crate, plus the
        # per-module dev-shell builds. Assigning these is the consumer's job by
        # construction — dropping the line is a visible omission, which is why
        # mkRustProject hands all three back together.
        inherit (rust) checks;

        # `stepv` is the CLI alone (what crates.io ships); `default` is the
        # product: the CLI with the kernel in libexec/stepv, where
        # occt::kernel_path() finds it relative to the binary.
        packages =
          rust.packages
          // {
            inherit kernel;
            # Copied, not symlinked: on Linux current_exe() resolves symlinks,
            # so a symlinked bin/stepv would look for libexec/ inside the
            # CLI-only store path and never find the kernel.
            default =
              pkgs.runCommand "stepv-${kernel.version}"
                {
                  # `stepv view` dlopens its window and GPU libraries at run
                  # time (winit, wgpu): on Linux the wrapper puts nix's on the
                  # library path, so the product's viewer works from the
                  # store. On NixOS the drivers come from /run/opengl-driver;
                  # elsewhere the loader reads the host's ICD files and loads
                  # host drivers against nix's glibc, which may fail (the
                  # nixGL problem): the viewer then falls back to software.
                  nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
                    pkgs.makeWrapper
                  ];
                  meta.mainProgram = "stepv";
                  meta.description = "STEP/IGES/BREP previews and thumbnails: the CLI plus its OCCT kernel";
                }
                ''
                    mkdir -p $out/bin $out/libexec/stepv
                    cp ${rust.packages.stepv}/bin/stepv $out/bin/stepv
                    ${pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
                      # The real binary stays in bin/ (.stepv-wrapped), so
                      # occt::kernel_path() still finds ../libexec/stepv.
                      wrapProgram $out/bin/stepv --prefix LD_LIBRARY_PATH : ${
                        pkgs.lib.makeLibraryPath [
                          pkgs.vulkan-loader
                          pkgs.libGL
                          pkgs.libxkbcommon
                          pkgs.wayland
                          pkgs.libx11
                          pkgs.libxcursor
                          pkgs.libxrandr
                          pkgs.libxi
                        ]
                      }
                    ''}
                    cp ${kernel}/libexec/stepv/stepv-occt $out/libexec/stepv/stepv-occt
                    # Linux desktop integration (S4): harmless elsewhere.
                    install -Dm644 ${./packaging/linux/stepv.thumbnailer} $out/share/thumbnailers/stepv.thumbnailer
                    install -Dm644 ${./packaging/linux/stepv-mime.xml} $out/share/mime/packages/stepv.xml
                    install -Dm644 ${./packaging/linux/stepv.desktop} $out/share/applications/stepv.desktop
                  # Licence obligations travel with the binary (NOTICE; OCCT's LGPL + exception).
                  install -Dm644 ${./NOTICE} $out/share/doc/stepv/NOTICE
                  install -Dm644 ${./LICENSE} $out/share/doc/stepv/LICENSE
                  install -Dm644 ${./licenses/OCCT-LGPL-2.1.txt} $out/share/doc/stepv/licenses/OCCT-LGPL-2.1.txt
                  install -Dm644 ${./licenses/OCCT-LGPL-EXCEPTION-1.0.txt} $out/share/doc/stepv/licenses/OCCT-LGPL-EXCEPTION-1.0.txt
                  # The viewer's embedded fonts (#28).
                  for f in OFL-1.1 Ubuntu-font-1.0 Hack-font; do
                    install -Dm644 ${./licenses}/$f.txt $out/share/doc/stepv/licenses/$f.txt
                  done
                '';
          }
          // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            # Dolphin / KIO thumbnail plugin (S4). Linux only: KF6 does not
            # build on darwin in nixpkgs. Runs the `stepv` CLI from PATH.
            kde-thumbnailer = pkgs.stdenv.mkDerivation {
              pname = "stepv-kde-thumbnailer";
              inherit (kernel) version;
              src = ./packaging/kde;
              nativeBuildInputs = [
                pkgs.cmake
                pkgs.kdePackages.extra-cmake-modules
              ];
              buildInputs = [
                pkgs.kdePackages.kio
                pkgs.kdePackages.qtbase
              ];
              dontWrapQtApps = true;
            };
          };

        # Opt-in local dev services (#795): a daemonless process-compose stack
        # (Postgres, SeaweedFS/S3, Redis, …) with service versions from the
        # pinned vigos nixpkgs — no Docker/Podman daemon, no extra flake
        # inputs. Uncomment, then `nix run .#services` (or enable the
        # `services` recipe in justfile.project); service state lands in
        # ./data — add it to .gitignore.
        #
        #   packages.services = vigos.lib.mkProjectServices {
        #     inherit pkgs;
        #     modules = [ { services.postgres."db".enable = true; } ];
        #   };

        # Future (upstream, opt-in): vigos may expose modular language shells —
        # e.g. `vigos.devShells.${system}.{cpp,geant4,dataAnalysis}` — that you
        # select without changing this scaffold. Out of scope today.
      }
    );
}
