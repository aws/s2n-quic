package com.aws;

import software.constructs.Construct;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;

public class StateMachineStack extends Stack {
    public StateMachineStack(final Construct parent, final String id) {
        this(parent, id, null);
    }

    public StateMachineStack(final Construct parent, final String id, final StackProps props) {
        super(parent, id, props);
    }
}                                         

