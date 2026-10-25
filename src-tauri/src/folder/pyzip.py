# Fallback packer for servers without `zip`:
#   python3 pyzip.py <folder name> <archive> [exclude pattern...]
# Run from the folder's parent so entries are "<name>/...", like `zip -r -y`. Keeps Unix
# permissions and stores symlinks as links. Exclude patterns match archive paths the way
# `zip -x` does (`*` also matches `/`); an excluded folder is not entered. Unreadable files
# are skipped with a warning (exit 18, like zip).
import fnmatch, os, stat, sys, zipfile

name, out, excludes = sys.argv[1], sys.argv[2], sys.argv[3:]
skipped = 0


def excluded(path):
    path = path.replace(os.sep, "/")  # archive paths use "/" everywhere
    return any(fnmatch.fnmatchcase(path, p) for p in excludes)


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
        dirs[:] = sorted(d for d in dirs if not excluded(os.path.join(root, d)))
        for n in dirs + sorted(files):
            path = os.path.join(root, n)
            if not excluded(path):
                add(z, path)

sys.exit(18 if skipped else 0)
