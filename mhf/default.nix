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
  debugFeature = lib.optionalString config.development.mhf.debug.enable ",debug";
  workbenchFeature = lib.optionalString config.development.mhf.workbench.enable ",workbench";
  command =
    name: text:
    mkCommand {
      inherit name text;
      directory = "mhf";
    };
  windowsCommand =
    {
      name,
      buildCommand,
      needsGame ? true,
    }:
    mkCommand {
      inherit name;
      text = ''
        ${lib.optionalString needsGame ''
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
        ''}
        hasConfigArg=false
        for argument in "$@"; do
          case "$argument" in
            --config|--config=*|-c|-c?*) hasConfigArg=true; break ;;
            --) break ;;
          esac
        done
        if [[ "$hasConfigArg" == false && ! -v MHF_CONFIG ]]; then
          export MHF_CONFIG="$PWD/mhf.toml"
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
          if [[ -v MHF_CONFIG ]]; then
            MHF_CONFIG="$(windowsPath "$MHF_CONFIG")"
          fi
          ${lib.optionalString needsGame ''gameDirectory="$(windowsPath "$gameDirectory")"''}
          # WSL requires explicit forwarding of Linux environment variables.
          for variable in "''${!MHF_@}"; do
            case ":''${WSLENV:-}:" in
              *":$variable:"*|*":$variable/"*) ;;
              *) WSLENV="''${WSLENV:+$WSLENV:}$variable/w" ;;
            esac
          done
          export WSLENV
        fi
        configArgs=()
        if [[ "$hasConfigArg" == false && -v MHF_CONFIG ]]; then
          configArgs=(--config "$MHF_CONFIG")
        fi
        ${getExe buildCommand}
        targetDirectory="''${CARGO_TARGET_DIR:-target}"
        if [[ "$targetDirectory" != /* ]]; then
          targetDirectory="$PROJECT_ROOT/mhf/$targetDirectory"
        fi
        exec "''${runner[@]}" \
          "$targetDirectory/i686-pc-windows-msvc/release/${name}.exe" \
          "''${configArgs[@]}" ${lib.optionalString needsGame ''--game-dir "$gameDirectory"''} "$@"
      '';
    };

in
{
  imports = [ ./config.nix ];

  options.development.mhf = {
    debug.enable = lib.mkEnableOption "MHF offline debugging tools and control window" // {
      default = true;
    };
    workbench.enable = lib.mkEnableOption "MHF resource workbench" // {
      default = true;
    };
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
      mhf-ai-decompile-build = mkDefault (
        command "mhf-ai-decompile-build" ''
          exec cargo build -p mhf-ai-decompile --release \
            --target i686-pc-windows-msvc --locked "$@"
        ''
      );
      mhf-ai-decompile = mkDefault (windowsCommand {
        name = "mhf-ai-decompile";
        buildCommand = config.development.commands.mhf-ai-decompile-build;
      });
      mhf-mods-build = mkDefault (
        command "mhf-mods-build" ''
          exec cargo build -p mhf-mod-manager --bin mhf-mods --release \
            --no-default-features --features gui,login${debugFeature}${workbenchFeature} \
            --target i686-pc-windows-msvc --locked "$@"
        ''
      );
      # Both runtime commands preserve the caller's directory and configuration.
      mhf-mods = mkDefault (windowsCommand {
        name = "mhf-mods";
        buildCommand = config.development.commands.mhf-mods-build;
        needsGame = false;
      });
      mhf-build = mkDefault (
        command "mhf-build" ''
          exec cargo build -p mhf-launcher --bin mhf-launcher --release \
            --no-default-features --features login${debugFeature}${workbenchFeature} \
            --target i686-pc-windows-msvc --locked "$@"
        ''
      );
      mhf-launcher = mkDefault (windowsCommand {
        name = "mhf-launcher";
        buildCommand = config.development.commands.mhf-build;
      });
    };
    settings.processes.mhf-launcher = {
      command = mkDefault "exec ${getExe config.development.commands.mhf-launcher}";
      disabled = mkDefault true;
    };
  };
}
