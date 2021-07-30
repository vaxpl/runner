runner
======

Simple runner for multiple platforms.

Run target on on windows os from linux
--------------------------------------

Run the runner on your windows os:

```cmd
runner --secret DQk0OLaZ
```

Setup the the runner script `runner-x86_64-pc-windows-gnu.sh` on your linux os:

```sh
#!/bin/bash

RUNNER_HOST=${RUNNER_HOST:=10.0.6.232}
RUNNER_PORT=${RUNNER_PORT:=9677}
RUNNER_SECRET=${RUNNER_SECRET:=DQk0OLaZ}

FILE=$1
NAME=$(basename ${FILE})
SIZE=$(stat -c %s ${FILE})

shift
ARGS=$*

(echo "SECRET:${RUNNER_SECRET}"; \
echo "FILE:${NAME}|${SIZE}"; \
cat ${FILE}; echo "EXEC:cmd /c ${NAME} ${ARGS}"; \
echo "REMOVE:${NAME}"; \
echo "EXIT!"
) | nc ${RUNNER_HOST} ${RUNNER_PORT}
```

Now execute your win32 program with runner:

```sh
runner-x86_64-pc-windows-gnu.sh xxx.exe
```
