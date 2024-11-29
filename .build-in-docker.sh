#!/bin/bash
docker build --target export-stage --output type=local,dest=./output .
