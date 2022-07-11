// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.Stack;
import software.amazon.awscdk.services.stepfunctions.StateMachine;
import software.constructs.Construct;

public class StateMachineStack extends Stack {

    public StateMachineStack(final Construct parent, final String id, final StateMachineStackProps props) {
        super(parent, id, props);

        StateMachine stateMachine = StateMachine.Builder.create(this, "ecs-state-machine")
            .definition(props.getClientTask())
            .build();
    }
}                                         

