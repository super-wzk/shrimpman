{
  config,
  lib,
  pkgs,
  mkCommand,
  shellPath,
  ...
}:
let
  inherit (lib) getExe mkDefault;
  host = pkgs.stdenv.hostPlatform;
  llvm = pkgs.llvmPackages;
  windowsSdk = pkgs.windows.sdk.overrideAttrs (previous: {
    src = previous.src.overrideAttrs {
      xwinArgs = [
        "--manifest=${pkgs.path}/pkgs/os-specific/windows/msvcSdk/manifest.json"
        "--accept-license"
        "--cache-dir=${placeholder "out"}"
        "--arch=x86"
        "download"
      ];
      outputHash = (lib.importJSON (pkgs.path + "/pkgs/os-specific/windows/msvcSdk/hashes.json")).x86;
      passthru.arch = "x86";
    };
  });
  windowsCFlags = lib.concatStringsSep " " [
    "/imsvc${windowsSdk}/crt/include"
    "/imsvc${windowsSdk}/sdk/include/ucrt"
    "/imsvc${windowsSdk}/sdk/include/shared"
    "/imsvc${windowsSdk}/sdk/include/um"
  ];
  command =
    name: text:
    mkCommand {
      inherit name text;
      directory = "mhf";
    };
  launcherCommand =
    name: buildCommand:
    command name ''
      # shellcheck disable=SC2016,SC2089
      gameDirectory=${lib.escapeShellArg config.development.mhf.gameDirectory}
      if [[ -z "$gameDirectory" ]]; then
        echo "Set development.mhf.gameDirectory in the local Nix module" >&2
        exit 2
      fi
      case "$gameDirectory" in
        /*|[[:alpha:]]:*|\\\\*) ;;
        *) gameDirectory="$PROJECT_ROOT/$gameDirectory" ;;
      esac
      if [[ ! -v MHF_CONFIG ]]; then
        # shellcheck disable=SC2016
        configDirectory=${shellPath config.development.stateDirectory}/config
        ${pkgs.coreutils}/bin/mkdir -p "$configDirectory"
        export MHF_CONFIG="$configDirectory/mhf.toml"
        generatedSnapshot="$configDirectory/mhf.generated.toml"
        # Compare the generated snapshot, since the game writes to MHF_CONFIG.
        if [[ ! -e "$MHF_CONFIG" ]] || ! ${pkgs.diffutils}/bin/cmp -s ${
          config.generatedFiles."mhf/mhf.toml"
        } "$generatedSnapshot"; then
          ${pkgs.coreutils}/bin/install -m 600 ${config.generatedFiles."mhf/mhf.toml"} "$MHF_CONFIG"
          ${pkgs.coreutils}/bin/install -m 600 ${
            config.generatedFiles."mhf/mhf.toml"
          } "$generatedSnapshot"
        fi
      fi
      if [[ ! -v WINEPREFIX ]]; then
        # shellcheck disable=SC2016
        export WINEPREFIX=${shellPath config.development.stateDirectory}/wine
      fi
      runner=()
      wslInterop=false
      ${lib.optionalString host.isLinux ''
        if [[ -r /proc/sys/fs/binfmt_misc/WSLInterop ]]; then
          read -r interopStatus < /proc/sys/fs/binfmt_misc/WSLInterop
          if [[ "$interopStatus" == enabled ]]; then
            wslInterop=true
          fi
        fi
      ''}
      ${
        if config.development.mhf.runner == null then
          ''
            if [[ "$wslInterop" == false ]]; then
              runner=(wine)
            fi
          ''
        else
          lib.optionalString (config.development.mhf.runner != "") ''
            runner=(${lib.escapeShellArg config.development.mhf.runner})
          ''
      }

      if [[ "$wslInterop" == true && ''${#runner[@]} == 0 ]]; then
        windowsPath() {
          case "$1" in
            [[:alpha:]]:*|\\\\*) printf '%s\n' "$1" ;;
            /*) wslpath -w "$1" ;;
            *) wslpath -w "$PWD/$1" ;;
          esac
        }
        MHF_CONFIG="$(windowsPath "$MHF_CONFIG")"
        gameDirectory="$(windowsPath "$gameDirectory")"
        # WSL requires explicit forwarding of Linux environment variables.
        for variable in "''${!MHF_@}"; do
          case ":''${WSLENV:-}:" in
            *":$variable:"*|*":$variable/"*) ;;
            *) WSLENV="''${WSLENV:+$WSLENV:}$variable/w" ;;
          esac
        done
        export WSLENV
      fi
      ${getExe buildCommand}
      exec "''${runner[@]}" \
        "''${CARGO_TARGET_DIR:-target}/i686-pc-windows-msvc/release/${name}.exe" \
        --config "$MHF_CONFIG" --game-dir "$gameDirectory" "$@"
    '';

in
{
  imports = [ ./config.nix ];

  options.development.mhf = {
    gameDirectory = lib.mkOption {
      type = lib.types.str;
      default = "";
      description = "Game directory, absolute or relative to PROJECT_ROOT.";
    };
    runner = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "null selects Wine or WSL automatically; an empty string runs the EXE directly.";
    };
  };
  config = {
    development.packages = [
      pkgs.llvmPackages.clang
      pkgs.llvmPackages.lld
      pkgs.llvmPackages.llvm
    ]
    ++ lib.optionals (host.system == "x86_64-linux") [
      pkgs.wineWow64Packages.stable
    ];
    development.environment = {
      CC_i686_pc_windows_msvc = "${llvm.clang-unwrapped}/bin/clang-cl";
      CXX_i686_pc_windows_msvc = "${llvm.clang-unwrapped}/bin/clang-cl";
      AR_i686_pc_windows_msvc = "${llvm.llvm}/bin/llvm-lib";
      RANLIB_i686_pc_windows_msvc = "${llvm.llvm}/bin/llvm-ranlib";
      CFLAGS_i686_pc_windows_msvc = windowsCFlags;
      CXXFLAGS_i686_pc_windows_msvc = windowsCFlags;
      CARGO_TARGET_I686_PC_WINDOWS_MSVC_LINKER = "${llvm.lld}/bin/lld-link";
      CARGO_TARGET_I686_PC_WINDOWS_MSVC_RUSTFLAGS = lib.concatStringsSep " " [
        "-Lnative=${windowsSdk}/crt/lib/x86"
        "-Lnative=${windowsSdk}/sdk/lib/um/x86"
        "-Lnative=${windowsSdk}/sdk/lib/ucrt/x86"
      ];
    };
    development.commands = {
      mhf-build = mkDefault (
        command "mhf-build" ''
          exec cargo build -p shrimpman-mhf-launcher --bin mhf-launcher --release \
            --no-default-features --features login \
            --target i686-pc-windows-msvc --locked "$@"
        ''
      );
      mhf-debug-build = mkDefault (
        command "mhf-debug-build" ''
          exec cargo build -p shrimpman-mhf-launcher --bin mhf-debug-launcher --release \
            --no-default-features --features debug \
            --target i686-pc-windows-msvc --locked "$@"
        ''
      );
      mhf-debug-launcher = mkDefault (
        launcherCommand "mhf-debug-launcher" config.development.commands.mhf-debug-build
      );
      mhf-launcher = mkDefault (launcherCommand "mhf-launcher" config.development.commands.mhf-build);
    };
    settings.processes.mhf-debug-launcher = {
      command = mkDefault "exec ${getExe config.development.commands.mhf-debug-launcher}";
      disabled = mkDefault true;
    };
    settings.processes.mhf-launcher = {
      command = mkDefault "exec ${getExe config.development.commands.mhf-launcher}";
      disabled = mkDefault true;
    };
  };
}
