import importlib.util,json,pathlib,tempfile,unittest,uuid
source=pathlib.Path(__file__).resolve().parents[1]/'crates/runloom-node/support/fork-native.py'
spec=importlib.util.spec_from_file_location('adapter',source);adapter=importlib.util.module_from_spec(spec);spec.loader.exec_module(adapter)
class Materialization(unittest.TestCase):
 def test_nested_history_respects_each_boundary(self):
  with tempfile.TemporaryDirectory() as tmp:
   root=pathlib.Path(tmp);parent=str(uuid.uuid4());child=str(uuid.uuid4());grandchild=str(uuid.uuid4())
   def meta(i,ordinal,base=None):return {'ordinal':ordinal,'type':'session_meta','payload':{'id':i,**({'history_base':base} if base else {})}}
   def message(n,text):return {'ordinal':n,'type':'response_item','payload':{'type':'message','role':'user','content':[{'type':'input_text','text':text}]}}
   def save(i,rows):(root/(i+'.jsonl')).write_text(''.join(json.dumps(r)+'\n' for r in rows))
   a=message(1,'BEFORE');late=message(2,'MUST_NOT_LEAK');b=message(3,'CHILD');c=message(5,'GRANDCHILD')
   save(parent,[meta(parent,0),a,late])
   save(child,[meta(child,2,{'thread_id':parent,'end_ordinal_exclusive':2}),b,message(4,'CHILD_LATER')])
   rows=[meta(grandchild,4,{'thread_id':child,'end_ordinal_exclusive':4}),c]
   self.assertEqual(adapter.materialize_codex(rows,root),[a,b,c])
   self.assertEqual(adapter.materialize_codex(rows,root,2),[a])
 def test_missing_or_cyclic_dependency_fails(self):
  with tempfile.TemporaryDirectory() as tmp:
   root=pathlib.Path(tmp);sid=str(uuid.uuid4());missing=str(uuid.uuid4())
   rows=[{'type':'session_meta','payload':{'id':sid,'history_base':{'thread_id':missing,'end_ordinal_exclusive':1}}}]
   with self.assertRaises(FileNotFoundError):adapter.materialize_codex(rows,root)
   rows[0]['payload']['history_base']['thread_id']=sid
   (root/(sid+'.jsonl')).write_text(json.dumps(rows[0]))
   with self.assertRaises(RuntimeError):adapter.materialize_codex(rows,root)
unittest.main()
