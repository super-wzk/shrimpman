{
  config,
  lib,
  pkgs,
  mkCommand,
  shellPath,
  ...
}:
let
  inherit (lib)
    getExe
    mkDefault
    mkOption
    types
    ;
  ports = lib.mapAttrs (_: toString) config.development.ports;
  command =
    name: text:
    mkCommand {
      inherit name text;
      directory = "shrimpman";
    };
  service = name: {
    command = mkDefault "exec ${getExe (command name "exec cargo run --locked -p ${name}")}";
    depends_on.etcd.condition = mkDefault "process_healthy";
    shutdown = {
      signal = mkDefault 2;
      timeout_seconds = mkDefault 10;
    };
  };
in
{
  imports = [ ./config.nix ];

  options.development.ports =
    lib.mapAttrs
      (
        _: default:
        mkOption {
          type = types.port;
          inherit default;
          description = "Development service port.";
        }
      )
      {
        etcdClient = 2379;
        etcdPeer = 2380;
        signTcp = 53000;
        signHttp = 53001;
        entranceTcp = 53002;
        worldLand1 = 54001;
        worldLand2 = 54002;
      };
  config = {
    development.environment.PROJECT_CONFIG = mkDefault (
      toString config.generatedFiles."shrimpman/config.toml"
    );
    development.packages = [
      pkgs.etcd
      pkgs.protobuf
    ];

    development.commands = {
      shrimpman-build = mkDefault (
        command "shrimpman-build" ''
          exec cargo build --locked --workspace "$@"
        ''
      );
      shrimpman-db = mkDefault (
        command "shrimpman-db" ''
          exec cargo run --locked -p shrimpman-persistence --bin toasty -- "$@"
        ''
      );
      shrimpman-migrate = mkDefault (
        command "shrimpman-migrate" ''
          exec ${getExe config.development.commands.shrimpman-db} migration apply "$@"
        ''
      );
    };

    settings.processes = {
      etcd = {
        command = mkDefault "exec ${
          getExe (mkCommand {
            name = "development-etcd";
            text = ''
              # shellcheck disable=SC2016
              export ETCD_DATA_DIR=${shellPath config.development.stateDirectory}/etcd
              exec ${pkgs.etcd}/bin/etcd
            '';
          })
        }";
        environment = lib.mapAttrs (_: mkDefault) {
          ETCD_NAME = "shrimpman";
          ETCD_LISTEN_CLIENT_URLS = "http://127.0.0.1:${ports.etcdClient}";
          ETCD_ADVERTISE_CLIENT_URLS = "http://127.0.0.1:${ports.etcdClient}";
          ETCD_LISTEN_PEER_URLS = "http://127.0.0.1:${ports.etcdPeer}";
          ETCD_INITIAL_ADVERTISE_PEER_URLS = "http://127.0.0.1:${ports.etcdPeer}";
          ETCD_INITIAL_CLUSTER = "shrimpman=http://127.0.0.1:${ports.etcdPeer}";
          ETCD_INITIAL_CLUSTER_STATE = "new";
          ETCD_INITIAL_CLUSTER_TOKEN = "shrimpman";
        };
        readiness_probe = {
          exec.command = mkDefault "${pkgs.etcd}/bin/etcdctl --endpoints=http://127.0.0.1:${ports.etcdClient} endpoint health";
          period_seconds = mkDefault 1;
          timeout_seconds = mkDefault 5;
        };
        availability.restart = mkDefault "exit_on_failure";
      };
      shrimpman-migrate = {
        command = mkDefault "exec ${getExe config.development.commands.shrimpman-migrate}";
        availability.restart = mkDefault "exit_on_failure";
      };
      shrimpman-sign = lib.mkMerge [
        (service "shrimpman-sign")
        {
          depends_on.shrimpman-migrate.condition = mkDefault "process_completed_successfully";
        }
      ];
      shrimpman-entrance = service "shrimpman-entrance";
      shrimpman-world = lib.mkMerge [
        (service "shrimpman-world")
        {
          depends_on.shrimpman-migrate.condition = mkDefault "process_completed_successfully";
        }
      ];
    };
  };
}
