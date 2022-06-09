package com.aws;

import software.constructs.Construct;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;

public class ServerStack extends Stack {
    public ServerStack(final Construct parent, final String id) {
        this(parent, id, null);
    }

    public ServerStack(final Construct parent, final String id, final StackProps props) {
        super(parent, id, props);
    }
}                                         

