pkg:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.mev-rs;

  mev-rs = pkg.mev-rs pkgs.system;

  name = "mev-${cfg.enable}-rs";

  cmd = ''
    ${mev-rs}/bin/mev \
    ${cfg.enable} \
    ${cfg.config-file}
  '';
in
{
  options.services.mev-rs = {
    enable = lib.mkOption {
      type = lib.types.enum [ "boost" "build" ];
      description = "which subcommand of `mev-rs` to run";
    };
    config-file = lib.mkOption {
      type = lib.types.str;
      description = ''
        path to a config file suitable for the `mev-rs` toolkit
      '';
    };
  };

  config = {
    networking.firewall = lib.mkIf (cfg.enable == "build") {
      allowedTCPPorts = [ 30303 ];
      allowedUDPPorts = [ 30303 ];
    };

    environment.systemPackages = [
      mev-rs
    ];

    systemd.services."${name}" = {
      description = name;
      wantedBy = [ "multi-user.target" ];
      after = [ "vc.service" ];
      serviceConfig = {
        ExecStart = cmd;
        Restart = "on-failure";
        RestartSec = "10s";
        SyslogIdentifier = name;
      };
    };
  };
}
