#!/bin/bash
getent hosts host.docker.internal | awk '{ print $1 }'
