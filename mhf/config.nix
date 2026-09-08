{
  config,
  lib,
  pkgs,
  generateToml,
  ...
}:
let
  inherit (lib) mkDefault mkOption;
  toml = pkgs.formats.toml { };
  chatProfile = lib.listToAttrs (
    lib.concatMap (index: [
      {
        name = "PHINF_ID_${toString index}";
        value = if index == 0 then "111111" else "0";
      }
      {
        name = "PHINF_NAME_${toString index}";
        value = if index == 0 then "" else "0";
      }
    ]) (lib.range 0 29)
  );
in
{
  options.mhf = mkOption {
    type = toml.type;
    default = { };
    description = "MHF TOML settings, overridable by local modules.";
  };

  config = {
    mhf = lib.mapAttrsRecursive (_: mkDefault) {
      CHATPHI_211111 = chatProfile;
      CHATPHI_911111 = chatProfile;
      CHAT_SIZE = {
        RECEIVE_0 = "0";
        RECEIVE_1 = "256";
      };
      font = {
        name = "JetBrains Maple Mono NF NL HT";
        quality = "antialiased";
        weight = 600;
      };
      launch = {
        proxy_address = "127.0.0.1";
        proxy_configured = true;
        proxy_port = 8888;
        server_selection = 1;
        use_ie_proxy = false;
        use_proxy = false;
      };
      localization = {
        language = "japanese";
      };
      option = {
        "90C_GRAPHIC_ANTI_ALIASING" = "1";
        "90C_GRAPHIC_ANTI_ALIASING_WEIGHTSCALE" = "100";
        "90C_GRAPHIC_BGLIGHT_SHADOWATTENUATION" = "100";
        "90C_GRAPHIC_BLOOM" = "1";
        "90C_GRAPHIC_BLOOM_COLOR" = "100";
        "90C_GRAPHIC_BLOOM_DISPERSION" = "100";
        "90C_GRAPHIC_BLOOM_THRESHOLD" = "100";
        "90C_GRAPHIC_DOF" = "1";
        "90C_GRAPHIC_DOF_FARBLURSIZE" = "100";
        "90C_GRAPHIC_GAUSSIANBLUR_BLENDRATE" = "100";
        "90C_GRAPHIC_GAUSSIANBLUR_DISPERSION" = "100";
        "90C_GRAPHIC_GODRAY" = "1";
        "90C_GRAPHIC_PLLIGHT_SHADOWATTENUATION" = "100";
        "90C_GRAPHIC_SHADOWMAP_COLOR" = "100";
        "90C_GRAPHIC_SHADOW_LOBBY" = "0";
        "90C_GRAPHIC_SHADOW_QUEST" = "1";
        "90C_GRAPHIC_SOFTPARTICLE" = "1";
        "90C_GRAPHIC_SSAO" = "0";
        "90C_GRAPHIC_SSAO_DENSITY" = "100";
        "90C_GRAPHIC_TYPE" = "1";
        APM_00 = "0";
        APM_01 = "0";
        APM_02 = "0";
        APM_03 = "0";
        AUTOUSE_ONOFF = "1";
        BOWGUN_SHELL_LV1 = "0";
        CAMERA_BOW = "0";
        CAMERA_GUN = "0";
        CAMERA_GUN2 = "0";
        CAMERA_TYPE = "0";
        CAMERA_TYPE_ARCHER = "0";
        CAMERA_TYPE_BOWGUN = "0";
        CAM_AIMKEY_MODE = "0";
        CAM_PITCH_MODE = "0";
        CAM_PITCH_SPEED = "3";
        CHATLOGSIZE = "0";
        CHAT_KEY_TYPE = "0";
        DISP_CHAT_CHANNEL = "0";
        DISP_CTRL_ICON = "both";
        DISP_DIRECTION = "0";
        DISP_FACILITY_NAME_SIZE = "14";
        DISP_FKEY = "0";
        DISP_GAUGE_TYPE = "0";
        DISP_LBRANGE = "3";
        DISP_LBWEAPON = "0";
        DISP_NPC_NAME_SIZE = "14";
        DISP_PC_NAME_COLOR = "FFFFFF99";
        DISP_PC_TITLE = "on";
        EFF_LIGHT = "0";
        ESC_DIR_TYPE = "0";
        FKEY_TYPE = "0";
        FONT_SIZE = "1";
        FOSTA_ONOFF = "0";
        FOSTA_TALK_ONOFF = "0";
        GK_ACCOMPANY = "0";
        GUNNERCTRL = "0";
        HASYU_NAVI_ONOFF = "1";
        ICONDSP_ONOFF = "1";
        LBCAM_MANUAL = "1";
        LBCAM_ROT = "7";
        LBCAM_ZOOM = "3233";
        MATCHING_RANK_ONOFF = "0";
        MESSAGE_LOG_FILE = "anytime";
        MOUSE_ENABLE = "0";
        MOUSE_RANGE = "0";
        NAMEMASK = "0";
        ONESHOT_RASTA_ONOFF = "0";
        OPE_GUIDE = "0";
        OVERLAY_HOLD_ICON = "with";
        PLATE_DIRECTION = "1";
        PL_FACIAL = "0";
        SCOPECTRL = "0";
        SCREENSHOT_BBS_CAPTURE = "0";
        SCREENSHOT_BBS_COCKPIT = "0";
        SCREENSHOT_BBS_SHARE = "0";
        SOFTKEYBOARD = "0";
        S_PL_TRANS = "0";
        VIBRATION = "0";
        WALLPAPER_TRANSPARENCY = "0";
        ZPLATE_CHANGE = "0";
        clog_disabled = false;
        draw_skip = true;
      };
      screen = {
        bright = "-128";
        mode = "windowed";
        fullscreen_resolution = {
          height = 1200;
          width = 1920;
        };
        window_resolution = {
          height = 400;
          width = 400;
        };
      };
      set = {
        custom = true;
        preset_level = 0;
      };
      sound = {
        SOUND_BGM = "4";
        SOUND_SE = "4";
        SOUND_TYPE = "0";
        buffer_size = 2048;
        disabled = false;
        inactive_volume = 0;
        minimized_volume = 0;
        sample_rate = 48000;
        volume = 0;
      };
      video = {
        display_character_limit = 100;
        graphics_version = "high_definition";
        now_monitor_wh = true;
        use_dxt_textures = false;
      };
    };
    generatedFiles."mhf/mhf.toml" = generateToml "mhf.toml" config.mhf;
  };
}
