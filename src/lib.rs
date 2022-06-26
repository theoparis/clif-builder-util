use anyhow::Result;
use cranelift::{codegen::ir::FuncRef, prelude::*};
use cranelift_module::{DataContext, DataId, FuncId, Linkage, Module};
use std::collections::HashMap;

pub struct Compiler<M: Module> {
	pub module: M,
	data_id_counter: usize,
	var_id_counter: usize,
	vars: HashMap<String, DataId>,
	functions: HashMap<String, FuncId>,
}

impl<M: Module> Compiler<M> {
	pub fn new(module: M) -> Compiler<M> {
		Compiler {
			module,
			data_id_counter: 0,
			var_id_counter: 0,
			vars: HashMap::new(),
			functions: HashMap::new(),
		}
	}

	pub fn compile_func<F>(
		&mut self,
		name: &str,
		params: &[Type],
		ret: Option<Type>,
		linkage: Linkage,
		builder: F,
	) -> Result<FuncId>
	where
		F: Fn(&mut Compiler<M>, &mut FunctionBuilder, FuncId) -> Result<()>,
	{
		let mut sig = self.module.make_signature();

		for param in params {
			sig.params.push(AbiParam::new(*param));
		}

		if let Some(ret) = ret {
			sig.returns.push(AbiParam::new(ret));
		}

		let func_id = self.module.declare_function(name, linkage, &sig)?;

		let mut ctx = self.module.make_context();
		let mut fn_builder_ctx = FunctionBuilderContext::new();
		ctx.func = cranelift::codegen::ir::Function::with_name_signature(
			ExternalName::testcase(name),
			sig,
		);

		let mut f = FunctionBuilder::new(&mut ctx.func, &mut fn_builder_ctx);

		builder(self, &mut f, func_id)?;

		f.seal_all_blocks();
		f.finalize();

		cranelift::codegen::verifier::verify_function(
			&ctx.func,
			self.module.isa().flags(),
		)?;

		self.module.define_function(func_id, &mut ctx)?;

		self.functions.insert(name.to_owned(), func_id);

		Ok(func_id)
	}

	pub fn new_var(&mut self) -> Variable {
		let id = self.var_id_counter;
		self.var_id_counter += 1;
		Variable::new(id)
	}

	pub fn create_data(&mut self, data: Box<[u8]>) -> Result<DataId> {
		let data_id = self.module.declare_data(
			&format!("data_{}", {
				let id = self.data_id_counter;
				self.data_id_counter += 1;
				id
			}),
			Linkage::Local,
			false,
			false,
		)?;
		let mut ctx = DataContext::new();
		ctx.define(data);
		self.module.define_data(data_id, &ctx)?;

		Ok(data_id)
	}

	pub fn import_func(
		&mut self,
		name: &str,
		params: &[Type],
		ret: Option<Type>,
		f: &mut FunctionBuilder,
	) -> Result<FuncRef> {
		let mut sig = self.module.make_signature();

		for param in params {
			sig.params.push(AbiParam::new(*param));
		}

		if let Some(ret) = ret {
			sig.returns.push(AbiParam::new(ret));
		}

		let func = self.module.declare_function(name, Linkage::Import, &sig)?;

		Ok(self.module.declare_func_in_func(func, f.func))
	}

	pub fn create_var(&mut self, name: &str) -> Result<DataId> {
		let data_id =
			self.module
				.declare_data(name, Linkage::Local, true, false)?;
		let mut ctx = DataContext::new();
		ctx.define(Box::new([0; std::mem::size_of::<f64>()]));
		self.module.define_data(data_id, &ctx)?;
		self.vars.insert(name.to_owned(), data_id);

		Ok(data_id)
	}

	pub fn var_ptr(&mut self, name: &str, f: &mut FunctionBuilder) -> Value {
		let data_id = self.vars[name];
		let data_ref = self.module.declare_data_in_func(data_id, f.func);
		f.ins()
			.global_value(self.module.target_config().pointer_type(), data_ref)
	}

	pub fn load_var(
		&mut self,
		name: &str,
		var_type: Type,
		f: &mut FunctionBuilder,
	) -> Value {
		let ptr = self.var_ptr(name, f);
		f.ins().load(var_type, MemFlags::new(), ptr, 0)
	}

	pub fn store_var(
		&mut self,
		name: &str,
		val: Value,
		f: &mut FunctionBuilder,
	) {
		let ptr = self.var_ptr(name, f);
		f.ins().store(MemFlags::new(), val, ptr, 0);
	}
}
