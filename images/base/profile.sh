# /etc/profile.d/harness.sh
# Debian's /etc/profile overwrites PATH for login shells (bash -l), which drops
# the directories the image adds via ENV. Put them back.
case ":$PATH:" in
  *:/opt/harness-tools/bin:*) ;;
  *) PATH="/opt/harness-tools/bin:$HOME/.local/bin:$PATH" ;;
esac
export PATH
