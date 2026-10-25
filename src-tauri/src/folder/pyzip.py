# Fallback packer for servers without `zip`: python3 pyzip.py <folder name> <archive>.
# Run from the folder's parent so entries are "<name>/...", like `zip -r -y`. Keeps Unix
# permissions and stores symlinks as links. Unreadable files are skipped with a warning
# (exit 18, like zip).
import os, stat, sys, zipfile

name, out = sys.argv[1], sys.argv[2]
skipped = 0


def add(z, path):
    global skipped
    try:
        st = os.lstat(path)
        if stat.S_ISLNK(st.st_mode):
            info = zipfile.ZipInfo(path)
            info.create_system = 3
            info.external_attr = (st.st_mode & 0xFFFF) << 16
            z.writestr(info, os.readlink(path))
        else:
            z.write(path)
    except OSError as e:
        skipped += 1
        print("skipped %s: %s" % (path, e.strerror), file=sys.stderr)


with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, allowZip64=True) as z:
    add(z, name)
    for root, dirs, files in os.walk(name):
        for n in sorted(dirs) + sorted(files):
            add(z, os.path.join(root, n))

sys.exit(18 if skipped else 0)
