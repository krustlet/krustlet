PACKER_BINARY ?= packer
PACKER_VARIABLES := aws_region ami_name krustlet_version krustlet_src source_ami_id source_ami_owners arch instance_type security_group_id

aws_region ?= $(AWS_DEFAULT_REGION)
ami_name ?= amazon-eks-node-krustlet-$(KRUSTLET_VERSION)-v$(shell date +'%Y%m%d')
arch ?= x86_64
ifeq ($(arch), arm64)
instance_type ?= a1.large
else
instance_type ?= c5.2xlarge
endif

ifeq ($(aws_region), cn-northwest-1)
source_ami_owners ?= 141808717104
endif

KRUSTLET_VERSION ?= 0.1.0
krustlet_version ?= $(KRUSTLET_VERSION)
KRUSTLET_SRC ?= https://github.com/deislabs/krustlet/archive/v$(krustlet_version).tar.gz
krustlet_src ?= $(KRUSTLET_SRC)

T_RED := \e[0;31m
T_GREEN := \e[0;32m
T_YELLOW := \e[0;33m
T_RESET := \e[0m

.PHONY: all
all: 0.1.0

.PHONY: validate
validate:
	$(PACKER_BINARY) validate $(foreach packerVar,$(PACKER_VARIABLES), $(if $($(packerVar)),--var $(packerVar)='$($(packerVar))',)) eks-worker-al2.json

.PHONY: krustlet
krustlet: validate
	@echo "$(T_GREEN)Building AMI for Krustlet version $(T_YELLOW)$(KRUSTLET_VERSION)$(T_GREEN) on $(T_YELLOW)$(arch)$(T_RESET)"
	$(PACKER_BINARY) build $(foreach packerVar,$(PACKER_VARIABLES), $(if $($(packerVar)),--var $(packerVar)='$($(packerVar))',)) eks-worker-al2.json
 
.PHONY: 0.1.0
0.1.0:
	$(MAKE) krustlet KRUSTLET_VERSION=0.1.0
