# /etc/profile.d/harness.sh
# Debian's /etc/profile overwrites PATH for login shells (bash -l), which drops
# the directories the image and the instance add via ENV. Put them back:
# harness' tools, the folders of the project's apps (HARNESS_APPS_PATH, set by
# harness when it creates the instance), the agent's own installs.
case ":$PATH:" in
  *:/opt/harness-tools/bin:*) ;;
  *) PATH="/opt/harness-tools/bin:${HARNESS_APPS_PATH:+$HARNESS_APPS_PATH:}$HOME/.local/bin:$PATH" ;;
esac
export PATH
