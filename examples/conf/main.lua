return {
   pkgs.uwsm,
   pkgs.hyprland,
   {
      "bash",
      excludes = "*",
      links = {
         ["bashrc.sh"] = "~/.bashrc"
      }
   }
}
