return {
   "ly",
   pkgs.fish,
   {
      package_name = "hypr"
   },
   hypr = {
      name = "Hyprland",
      depends = {
         pkgs.fish,
         pkgs.neovim,
         "uwsm"
      },
      pkg = {
         arch = "hyprland",
      },
      exclude = "*",
   },
   git = {
      depends = {
         pkgs.hyprland,
      },
   },
}
