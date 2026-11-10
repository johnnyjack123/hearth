# /etc/profile.d/hearth.sh
# Debian's /etc/profile overwrites PATH for login shells (bash -l), which drops
# the directories the image and the instance add via ENV. Put them back:
# hearth's tools, the folders of the project's apps (HEARTH_APPS_PATH, set by
# hearth when it creates the instance), the agent's own installs.
case ":$PATH:" in
  *:/opt/hearth-tools/bin:*) ;;
  *) PATH="/opt/hearth-tools/bin:${HEARTH_APPS_PATH:+$HEARTH_APPS_PATH:}$HOME/.local/bin:$PATH" ;;
esac
export PATH
