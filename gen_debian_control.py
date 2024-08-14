#! /usr/bin/env python3

import jinja2
from dotenv import load_dotenv
import os

t = """\
Package: minimonagent
Version: {{ version }}-{{ release }}
Section: utils
Priority: optional
Architecture: {{arch}}
Pre-Depends: libc6
Maintainer: Joerg Beyer <joerg.beyer@gmail.com>
Description: This is the agent to collect disk usage data
 for minimon.

"""

if __name__ == '__main__':
    print('ok.')
    load_dotenv()

    version = os.environ["VERSION"]
    release = os.environ["RELEASE"]

    environment = jinja2.Environment()

    r = environment.from_string(t).render(arch="amd64", version=version, release=release)
    with open('DEBIAN/control', "w") as f:
        f.write(r)
