// Generated from ONNX "assets/ml/panns_cnn14_16k/panns_cnn14_16k.onnx" by burn-import
use burn::prelude::*;
use burn::nn::BatchNorm;
use burn::nn::BatchNormConfig;
use burn::nn::Linear;
use burn::nn::LinearConfig;
use burn::nn::LinearLayout;
use burn::nn::PaddingConfig2d;
use burn::nn::conv::Conv2d;
use burn::nn::conv::Conv2dConfig;
use burn::nn::pool::AvgPool2d;
use burn::nn::pool::AvgPool2dConfig;
use burn_store::BurnpackStore;
use burn_store::ModuleSnapshot;


#[derive(Module, Debug)]
pub struct Model<B: Backend> {
    batchnormalization1: BatchNorm<B>,
    conv2d1: Conv2d<B>,
    conv2d2: Conv2d<B>,
    averagepool2d1: AvgPool2d,
    conv2d3: Conv2d<B>,
    conv2d4: Conv2d<B>,
    averagepool2d2: AvgPool2d,
    conv2d5: Conv2d<B>,
    conv2d6: Conv2d<B>,
    averagepool2d3: AvgPool2d,
    conv2d7: Conv2d<B>,
    conv2d8: Conv2d<B>,
    averagepool2d4: AvgPool2d,
    conv2d9: Conv2d<B>,
    conv2d10: Conv2d<B>,
    averagepool2d5: AvgPool2d,
    conv2d11: Conv2d<B>,
    conv2d12: Conv2d<B>,
    averagepool2d6: AvgPool2d,
    linear1: Linear<B>,
    phantom: core::marker::PhantomData<B>,
    device: burn::module::Ignored<B::Device>,
}


impl<B: Backend> Default for Model<B> {
    fn default() -> Self {
        Self::from_file(
            "target/panns_gen/out/target/panns_gen/panns_cnn14_16k.bpk",
            &Default::default(),
        )
    }
}

impl<B: Backend> Model<B> {
    /// Load model weights from a burnpack file.
    pub fn from_file(file: &str, device: &B::Device) -> Self {
        let mut model = Self::new(device);
        let mut store = BurnpackStore::from_file(file);
        model.load_from(&mut store).expect("Failed to load burnpack file");
        model
    }
}

impl<B: Backend> Model<B> {
    #[allow(unused_variables)]
    pub fn new(device: &B::Device) -> Self {
        let batchnormalization1 = BatchNormConfig::new(64)
            .with_epsilon(0.000009999999747378752f64)
            .with_momentum(0.8999999761581421f64)
            .init(device);
        let conv2d1 = Conv2dConfig::new([1, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d2 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d1 = AvgPool2dConfig::new([2, 2])
            .with_strides([2, 2])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let conv2d3 = Conv2dConfig::new([64, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d4 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d2 = AvgPool2dConfig::new([2, 2])
            .with_strides([2, 2])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let conv2d5 = Conv2dConfig::new([128, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d6 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d3 = AvgPool2dConfig::new([2, 2])
            .with_strides([2, 2])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let conv2d7 = Conv2dConfig::new([256, 512], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d8 = Conv2dConfig::new([512, 512], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d4 = AvgPool2dConfig::new([2, 2])
            .with_strides([2, 2])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let conv2d9 = Conv2dConfig::new([512, 1024], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d10 = Conv2dConfig::new([1024, 1024], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d5 = AvgPool2dConfig::new([2, 2])
            .with_strides([2, 2])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let conv2d11 = Conv2dConfig::new([1024, 2048], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d12 = Conv2dConfig::new([2048, 2048], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d6 = AvgPool2dConfig::new([1, 1])
            .with_strides([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let linear1 = LinearConfig::new(2048, 2048)
            .with_bias(true)
            .with_layout(LinearLayout::Col)
            .init(device);
        Self {
            batchnormalization1,
            conv2d1,
            conv2d2,
            averagepool2d1,
            conv2d3,
            conv2d4,
            averagepool2d2,
            conv2d5,
            conv2d6,
            averagepool2d3,
            conv2d7,
            conv2d8,
            averagepool2d4,
            conv2d9,
            conv2d10,
            averagepool2d5,
            conv2d11,
            conv2d12,
            averagepool2d6,
            linear1,
            phantom: core::marker::PhantomData,
            device: burn::module::Ignored(device.clone()),
        }
    }

    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(&self, logmel: Tensor<B, 4>) -> Tensor<B, 2> {
        let transpose1_out1 = logmel.permute([0, 3, 2, 1]);
        let batchnormalization1_out1 = self.batchnormalization1.forward(transpose1_out1);
        let transpose2_out1 = batchnormalization1_out1.permute([0, 3, 2, 1]);
        let conv2d1_out1 = self.conv2d1.forward(transpose2_out1);
        let relu1_out1 = burn::tensor::activation::relu(conv2d1_out1);
        let conv2d2_out1 = self.conv2d2.forward(relu1_out1);
        let relu2_out1 = burn::tensor::activation::relu(conv2d2_out1);
        let averagepool2d1_out1 = self.averagepool2d1.forward(relu2_out1);
        let conv2d3_out1 = self.conv2d3.forward(averagepool2d1_out1);
        let relu3_out1 = burn::tensor::activation::relu(conv2d3_out1);
        let conv2d4_out1 = self.conv2d4.forward(relu3_out1);
        let relu4_out1 = burn::tensor::activation::relu(conv2d4_out1);
        let averagepool2d2_out1 = self.averagepool2d2.forward(relu4_out1);
        let conv2d5_out1 = self.conv2d5.forward(averagepool2d2_out1);
        let relu5_out1 = burn::tensor::activation::relu(conv2d5_out1);
        let conv2d6_out1 = self.conv2d6.forward(relu5_out1);
        let relu6_out1 = burn::tensor::activation::relu(conv2d6_out1);
        let averagepool2d3_out1 = self.averagepool2d3.forward(relu6_out1);
        let conv2d7_out1 = self.conv2d7.forward(averagepool2d3_out1);
        let relu7_out1 = burn::tensor::activation::relu(conv2d7_out1);
        let conv2d8_out1 = self.conv2d8.forward(relu7_out1);
        let relu8_out1 = burn::tensor::activation::relu(conv2d8_out1);
        let averagepool2d4_out1 = self.averagepool2d4.forward(relu8_out1);
        let conv2d9_out1 = self.conv2d9.forward(averagepool2d4_out1);
        let relu9_out1 = burn::tensor::activation::relu(conv2d9_out1);
        let conv2d10_out1 = self.conv2d10.forward(relu9_out1);
        let relu10_out1 = burn::tensor::activation::relu(conv2d10_out1);
        let averagepool2d5_out1 = self.averagepool2d5.forward(relu10_out1);
        let conv2d11_out1 = self.conv2d11.forward(averagepool2d5_out1);
        let relu11_out1 = burn::tensor::activation::relu(conv2d11_out1);
        let conv2d12_out1 = self.conv2d12.forward(relu11_out1);
        let relu12_out1 = burn::tensor::activation::relu(conv2d12_out1);
        let averagepool2d6_out1 = self.averagepool2d6.forward(relu12_out1);
        let reducemean1_out1 = {
            averagepool2d6_out1.mean_dim(3usize).squeeze_dims::<3usize>(&[3])
        };
        let reducemax1_out1 = {
            reducemean1_out1.clone().max_dim(2usize).squeeze_dims::<2usize>(&[2])
        };
        let reducemean2_out1 = {
            reducemean1_out1.mean_dim(2usize).squeeze_dims::<2usize>(&[2])
        };
        let add1_out1 = reducemax1_out1.add(reducemean2_out1);
        let linear1_out1 = self.linear1.forward(add1_out1);
        let relu13_out1 = burn::tensor::activation::relu(linear1_out1);
        relu13_out1
    }
}
