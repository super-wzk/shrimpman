{ config, lib, pkgs, generateToml, ... }:
let
  inherit (lib) mkDefault mkOption;
  toml = pkgs.formats.toml { };
  ports = lib.mapAttrs (_: toString) config.development.ports;
  leaseKv = {
    endpoints = [ "http://127.0.0.1:${ports.etcdClient}" ];
    lease_ttl = "15s";
    reconnect_delay = "1s";
  };
  databaseUrl = "sqlite://"
    + lib.optionalString (!(lib.hasPrefix "/" config.development.stateDirectory)) "../"
    + config.development.stateDirectory + "/shrimpman.sqlite3";
in
{
  options.shrimpman = mkOption {
    type = toml.type;
    default = { };
    description = "Service TOML configuration.";
  };

  config = {
    shrimpman = lib.mapAttrsRecursive (_: mkDefault) {
      migration = {
        database_url = databaseUrl;
      };
      entrance = {
        shutdown_timeout = "5s";
        lease_kv = leaseKv;
        logging = {
          filter = "warn,shrimpman_entrance=info,shrimpman_discovery=info,shrimpman_lease_kv=info,shrimpman_transport=debug";
        };
        server = {
          listen_addr = "0.0.0.0:${ports.entranceTcp}";
          advertise_addr = "127.0.0.1:${ports.entranceTcp}";
        };
      };
      sign = {
        auto_sign_up = true;
        shutdown_timeout = "5s";
        database = {
          url = databaseUrl;
        };
        http = {
          listen_addr = "0.0.0.0:${ports.signHttp}";
        };
        lease_kv = leaseKv;
        logging = {
          filter = "warn,shrimpman_sign=info,shrimpman_discovery=info,shrimpman_lease_kv=info";
        };
        session = {
          ttl = "5m";
        };
        server = {
          listen_addr = "0.0.0.0:${ports.signTcp}";
          advertise_addr = "127.0.0.1:${ports.signTcp}";
        };
      };
      world = {
        key = "main";
        address = "127.0.0.1";
        name = "Main World";
        description = "";
        world_type = "Beginner";
        season = "Breeding";
        content = "AllQuests";
        client_compatibility = "AllPlatforms";
        shutdown_timeout = "5s";
        database = {
          url = databaseUrl;
        };
        lease_kv = leaseKv;
        logging = {
          filter = "warn,shrimpman_world=info,shrimpman_discovery=info,shrimpman_lease_kv=info,shrimpman_transport=debug";
        };
        lands = [ {
          key = "land-1";
          listen_addr = "0.0.0.0:${ports.worldLand1}";
          port = config.development.ports.worldLand1;
          max_players = 100;
        } {
          key = "land-2";
          listen_addr = "0.0.0.0:${ports.worldLand2}";
          port = config.development.ports.worldLand2;
          max_players = 100;
        } ];
      };
    };
    generatedFiles."shrimpman/config.toml" = generateToml "config.toml" config.shrimpman;
  };
}
