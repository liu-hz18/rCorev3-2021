DOCKER_NAME ?= liu-hz18/rcore-labs
.PHONY: docker build_docker

docker:
	docker run --rm -it --mount type=bind,source=$(shell pwd),destination=/mnt ${DOCKER_NAME}

build_docker: 
	docker build -t ${DOCKER_NAME} .
