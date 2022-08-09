package com.aws;

import software.amazon.awscdk.App;
import software.amazon.awscdk.assertions.Template;
import java.io.IOException;
import org.junit.jupiter.api.Test;
import java.util.HashMap;
import software.amazon.awscdk.Environment;
import java.util.List;
import java.util.Map;

/* Unit testing for VpcStack, tests for key properties
 * in the cloudformation output of the stack.
 * Some empty maps and lists can be found to match
 * any value for fields in the Cloudformation output with
 * unimportant information.
 * More extensive tests are performed through deployments
 * of various scenarios and verifying outputs.
 */
public class VpcStackTest {
    private String cidr;
    private String region;
    private Template template;
    private VpcStack vpcStack;

    public VpcStackTest() {
      App app = new App();

      cidr = "11.0.0.0/16";
      region = "us-west-2";

      vpcStack = new VpcStack(app, "VpcStack", VpcStackProps.builder()
        .env(makeEnv(System.getenv("CDK_DEFAULT_ACCOUNT"), region))
        .cidr(cidr)
        .build());

      template = Template.fromStack(vpcStack);
    }

    @Test
    public void testStack() throws IOException {
      
        //Check Vpc 
        template.hasResourceProperties("AWS::EC2::VPC", new HashMap<String, Object>() {{
          put("CidrBlock", cidr);
          put("EnableDnsHostnames", true);
          put("EnableDnsSupport", true);
        }});

        //Check private VPC subnet
        template.hasResourceProperties("AWS::EC2::Subnet", new HashMap<String, Object>() {{
          put("Tags", List.of(
            Map.of("Key", "aws-cdk:subnet-name", "Value", "Private"),
            Map.of(),
            Map.of()
          ));
        }});

        //Check S3 Endpoint
        template.hasResourceProperties("AWS::EC2::VPCEndpoint", new HashMap<String, Object>() {{
          put("ServiceName", Map.of("Fn::Join", List.of("", 
              List.of("com.amazonaws.", Map.of("Ref", "AWS::Region"), ".s3"))));
          put("VpcEndpointType", "Gateway");
        }});
        
        //Check security group
        template.hasResourceProperties("AWS::EC2::SecurityGroupIngress", new HashMap<String, Object>() {{
          put("CidrIp", "0.0.0.0/0");
        }});

        //Check S3 bucket
        template.hasResource("AWS::S3::Bucket", new HashMap<String, Object>() {{
          put("DeletionPolicy", "Retain");
        }});
    }

    static Environment makeEnv(String account, String region) {
      return Environment.builder()
          .account(account)
          .region(region)
          .build();
    }
}
