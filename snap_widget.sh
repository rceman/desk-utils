#!/bin/bash
# screenshot the taskbar widget region via interactive-session scheduled task
powershell.exe -NoProfile -Command "schtasks /run /tn DiskTraySnap; Start-Sleep 2" >/dev/null 2>&1
python3 -c "
from PIL import Image
im = Image.open('/mnt/c/Users/therceman/full.png')
im.crop((2500, 1670, 3072, 1728)).resize((572*3, 58*3), Image.NEAREST).save('/tmp/w.png')
"
echo /tmp/w.png
