# Fallback unpacker for servers without `unzip`: python3 pyunzip.py <archive> <folder>.
# Unlike zipfile.extractall it restores Unix permissions and symlinks. Entries that would
# land outside <folder> (absolute or "..") are refused.
import os, shutil, stat, sys, zipfile

archive, dest = sys.argv[1], sys.argv[2]
dir_modes = []

with zipfile.ZipFile(archive) as z:
    for info in z.infolist():
        name = info.filename
        parts = name.split("/")
        if name.startswith("/") or ".." in parts or ":" in parts[0]:
            print("refused unsafe entry: %s" % name, file=sys.stderr)
            sys.exit(22)
        path = os.path.join(dest, *[p for p in parts if p])
        mode = info.external_attr >> 16
        if stat.S_ISLNK(mode):
            os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
            if os.path.lexists(path):
                os.remove(path)
            os.symlink(z.read(info).decode(), path)
            continue
        if name.endswith("/"):
            os.makedirs(path, exist_ok=True)
            if mode & 0o7777:
                dir_modes.append((path, mode & 0o7777))
            continue
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
        if os.path.islink(path):
            os.remove(path)
        with z.open(info) as src, open(path, "wb") as dst:
            shutil.copyfileobj(src, dst)
        if mode & 0o7777:
            os.chmod(path, mode & 0o7777)

# Folder permissions last, so a read-only folder does not block its own contents.
for path, mode in reversed(dir_modes):
    os.chmod(path, mode)
